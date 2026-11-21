//! Owns persisted crawl history, cancellation requests, and bounded result reads.

use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;
use uuid::Uuid;

use super::{Error, Result, crawl::Config};

/// Persisted counters are returned alongside the original, redacted settings.
#[derive(Serialize)]
pub struct Job {
    pub id: Uuid,
    pub status: String,
    pub config: Config,
    pub created_at: i64,
    pub finished_at: Option<i64>,
    pub pages_crawled: i64,
    pub assets_downloaded: i64,
}

#[derive(Serialize)]
pub struct Page {
    pub url: String,
    pub title: String,
    pub status_code: u16,
    pub depth: u32,
}

#[derive(Serialize)]
pub struct Asset {
    pub source_url: String,
    pub local_path: String,
    pub mime_type: String,
    pub size_bytes: i64,
}

#[derive(Serialize)]
pub struct Results {
    pub pages: Vec<Page>,
    pub assets: Vec<Asset>,
    pub pages_total: i64,
    pub assets_total: i64,
}

#[derive(Clone)]
pub struct Store {
    root: PathBuf,
}

const JOB_COLUMNS: &str = "id,status,config,created_at,finished_at,
    (SELECT count(*) FROM pages WHERE job_id=jobs.id),
    (SELECT count(*) FROM assets WHERE job_id=jobs.id)";

impl Store {
    /// Set up SQLite once per data directory, including concurrent CLI and desktop use.
    pub fn open(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(root.join("locks")).map_err(|_| Error::Storage)?;
        let store = Self { root };
        store
            .connection()?
            .execute_batch(
                "PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS jobs (
                id TEXT PRIMARY KEY, status TEXT NOT NULL, config TEXT NOT NULL,
                created_at INTEGER NOT NULL, finished_at INTEGER
            );
            CREATE TABLE IF NOT EXISTS pages (
                id INTEGER PRIMARY KEY, job_id TEXT NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
                url TEXT NOT NULL, title TEXT NOT NULL, status_code INTEGER NOT NULL,
                depth INTEGER NOT NULL, html TEXT NOT NULL, text_content TEXT NOT NULL,
                headers_json TEXT, UNIQUE(job_id, url)
            );
            CREATE TABLE IF NOT EXISTS assets (
                id INTEGER PRIMARY KEY, job_id TEXT NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
                source_url TEXT NOT NULL, local_path TEXT NOT NULL, mime_type TEXT NOT NULL,
                size_bytes INTEGER NOT NULL, UNIQUE(job_id, source_url)
            );",
            )
            .map_err(|_| Error::Storage)?;
        Ok(store)
    }

    fn connection(&self) -> Result<Connection> {
        let connection =
            Connection::open(self.root.join("crawler.db")).map_err(|_| Error::Storage)?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|_| Error::Storage)?;
        connection
            .pragma_update(None, "foreign_keys", true)
            .map_err(|_| Error::Storage)?;
        Ok(connection)
    }

    /// Hold an OS-backed lease for the job lifetime so another process can detect a crash.
    fn lease(&self, id: Uuid) -> Result<Option<File>> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.root.join("locks").join(id.to_string()))
            .map_err(|_| Error::Storage)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(file)),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(_)) => Err(Error::Storage),
        }
    }

    /// Write a job summary only after acquiring ownership, and keep proxy secrets session-local.
    pub fn create(&self, id: Uuid, config: &Config) -> Result<File> {
        let lease = self.lease(id)?.ok_or(Error::Busy)?;
        let config = serde_json::to_string(&config.redacted()).map_err(|_| Error::Storage)?;
        self.connection()?
            .execute(
                "INSERT INTO jobs(id,status,config,created_at) VALUES (?1,'running',?2,?3)",
                params![id.to_string(), config, now()],
            )
            .map_err(|_| Error::Storage)?;
        Ok(lease)
    }

    /// Recover abandoned jobs without disturbing a live CLI or desktop worker.
    pub fn list(&self) -> Result<Vec<Job>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(&format!(
                "SELECT {JOB_COLUMNS} FROM jobs ORDER BY created_at DESC, rowid DESC"
            ))
            .map_err(|_| Error::Storage)?;
        let mut jobs = statement
            .query_map([], job_from_row)
            .map_err(|_| Error::Storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|_| Error::Storage)?;
        for job in &mut jobs {
            if matches!(job.status.as_str(), "running" | "stopping")
                && let Some(_lease) = self.lease(job.id)?
            {
                if let Ok(current) = self.job(job.id) {
                    self.finish(current.id, "failed")?;
                    *job = self.job(current.id)?;
                }
            }
        }
        Ok(jobs)
    }

    pub fn job(&self, id: Uuid) -> Result<Job> {
        self.connection()?
            .query_row(
                &format!("SELECT {JOB_COLUMNS} FROM jobs WHERE id=?1"),
                [id.to_string()],
                job_from_row,
            )
            .optional()
            .map_err(|_| Error::Storage)?
            .ok_or(Error::NotFound)
    }

    /// A persisted stop request can be observed by a different process that owns the job.
    pub fn stop(&self, id: Uuid) -> Result<()> {
        let changed = self
            .connection()?
            .execute(
                "UPDATE jobs SET status='stopping' WHERE id=?1 AND status='running'",
                [id.to_string()],
            )
            .map_err(|_| Error::Storage)?;
        if changed == 0 {
            self.job(id)?;
        }
        Ok(())
    }

    pub fn stopping(&self, id: Uuid) -> Result<bool> {
        self.connection()?
            .query_row(
                "SELECT status FROM jobs WHERE id=?1",
                [id.to_string()],
                |row| Ok(row.get::<_, String>(0)? == "stopping"),
            )
            .map_err(|_| Error::Storage)
    }

    /// Let a stop that won the race with completion publish its terminal outcome.
    pub fn finish(&self, id: Uuid, status: &str) -> Result<()> {
        self.connection()?
            .execute(
                "UPDATE jobs SET status=CASE WHEN status='stopping' THEN 'stopped' ELSE ?2 END,
             finished_at=?3 WHERE id=?1 AND status IN ('running','stopping')",
                params![id.to_string(), status, now()],
            )
            .map_err(|_| Error::Storage)?;
        Ok(())
    }

    pub fn page(&self, id: Uuid, page: &super::crawl::CollectedPage) -> Result<()> {
        self.connection()?.execute(
            "INSERT OR IGNORE INTO pages(job_id,url,title,status_code,depth,html,text_content,headers_json)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![id.to_string(), page.url.as_str(), page.title, page.status, page.depth,
                page.html, page.text, page.headers_json],
        ).map_err(|_| Error::Storage)?;
        Ok(())
    }

    pub fn asset(&self, id: Uuid, asset: &Asset) -> Result<()> {
        self.connection()?
            .execute(
                "INSERT INTO assets(job_id,source_url,local_path,mime_type,size_bytes)
             VALUES (?1,?2,?3,?4,?5)",
                params![
                    id.to_string(),
                    asset.source_url,
                    asset.local_path,
                    asset.mime_type,
                    asset.size_bytes
                ],
            )
            .map_err(|_| Error::Storage)?;
        Ok(())
    }

    pub fn asset_dir(&self, id: Uuid) -> PathBuf {
        self.root.join("downloads").join(id.to_string())
    }

    /// Fetch independent result windows and their totals in the same read snapshot.
    pub fn results(&self, id: Uuid, pages: u32, assets: u32, limit: u32) -> Result<Results> {
        let mut db = self.connection()?;
        let read = db.transaction().map_err(|_| Error::Storage)?;
        let job = read
            .query_row(
                &format!("SELECT {JOB_COLUMNS} FROM jobs WHERE id=?1"),
                [id.to_string()],
                job_from_row,
            )
            .optional()
            .map_err(|_| Error::Storage)?
            .ok_or(Error::NotFound)?;
        let limit = limit.clamp(1, 200);
        let pages = read.prepare(
            "SELECT url,title,status_code,depth FROM pages WHERE job_id=?1 ORDER BY id LIMIT ?2 OFFSET ?3"
        ).map_err(|_| Error::Storage)?.query_map(params![id.to_string(),limit,pages], |r| Ok(Page {
            url:r.get(0)?, title:r.get(1)?, status_code:r.get(2)?, depth:r.get(3)?,
        })).map_err(|_| Error::Storage)?.collect::<rusqlite::Result<_>>().map_err(|_| Error::Storage)?;
        let assets = read.prepare(
            "SELECT source_url,local_path,mime_type,size_bytes FROM assets WHERE job_id=?1 ORDER BY id LIMIT ?2 OFFSET ?3"
        ).map_err(|_| Error::Storage)?.query_map(params![id.to_string(),limit,assets], |r| Ok(Asset {
            source_url:r.get(0)?, local_path:r.get(1)?, mime_type:r.get(2)?, size_bytes:r.get(3)?,
        })).map_err(|_| Error::Storage)?.collect::<rusqlite::Result<_>>().map_err(|_| Error::Storage)?;
        Ok(Results {
            pages,
            assets,
            pages_total: job.pages_crawled,
            assets_total: job.assets_downloaded,
        })
    }

    /// Remove the UUID-scoped files only after excluding active writers.
    pub fn delete(&self, id: Uuid) -> Result<()> {
        self.job(id)?;
        let _lease = self.lease(id)?.ok_or(Error::Busy)?;
        let directory = self.asset_dir(id);
        let staged = directory.with_file_name(format!(".deleting-{id}"));
        // Move first so a failed database operation can put the assets back.
        if directory.exists() {
            fs::rename(&directory, &staged).map_err(|_| Error::File)?;
        }
        let deleted = (|| {
            if self
                .connection()?
                .execute("DELETE FROM jobs WHERE id=?1", [id.to_string()])
                .map_err(|_| Error::Storage)?
                == 0
            {
                return Err(Error::NotFound);
            }
            Ok(())
        })();
        if staged.exists() {
            if deleted.is_ok() {
                fs::remove_dir_all(&staged).map_err(|_| Error::File)?;
            } else {
                fs::rename(&staged, &directory).map_err(|_| Error::File)?;
            }
        }
        deleted
    }

    /// Stream one database snapshot into a sibling temporary file before publishing it.
    pub fn export(&self, id: Uuid, destination: &Path) -> Result<()> {
        let mut db = self.connection()?;
        let read = db.transaction().map_err(|_| Error::Storage)?;
        let job = read
            .query_row(
                &format!("SELECT {JOB_COLUMNS} FROM jobs WHERE id=?1"),
                [id.to_string()],
                job_from_row,
            )
            .optional()
            .map_err(|_| Error::Storage)?
            .ok_or(Error::NotFound)?;
        let parent = destination
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|_| Error::File)?;
        {
            let mut output = BufWriter::new(temp.as_file_mut());
            output.write_all(b"{\"jobs\":[").map_err(|_| Error::File)?;
            serde_json::to_writer(&mut output, &job).map_err(|_| Error::File)?;
            output
                .write_all(b"],\"pages\":[")
                .map_err(|_| Error::File)?;
            write_rows(
                &read,
                &mut output,
                id,
                "SELECT json_object(
                'id',id,'job_id',job_id,'url',url,'title',title,'status_code',status_code,
                'depth',depth,'html',html,'text_content',text_content,'headers_json',headers_json
                ) FROM pages WHERE job_id=?1 ORDER BY id",
            )?;
            output
                .write_all(b"],\"assets\":[")
                .map_err(|_| Error::File)?;
            write_rows(&read, &mut output, id, "SELECT json_object(
                'id',id,'job_id',job_id,'source_url',source_url,'local_path',local_path,
                'mime_type',mime_type,'size_bytes',size_bytes) FROM assets WHERE job_id=?1 ORDER BY id")?;
            output.write_all(b"]}\n").map_err(|_| Error::File)?;
            output.flush().map_err(|_| Error::File)?;
        }
        temp.as_file().sync_all().map_err(|_| Error::File)?;
        temp.persist(destination).map_err(|_| Error::File)?;
        Ok(())
    }
}

/// Read just one serialized result at a time, including pages with large HTML bodies.
fn write_rows(db: &Connection, output: &mut impl Write, id: Uuid, sql: &str) -> Result<()> {
    let mut statement = db.prepare(sql).map_err(|_| Error::Storage)?;
    let rows = statement
        .query_map([id.to_string()], |row| row.get::<_, String>(0))
        .map_err(|_| Error::Storage)?;
    for (index, row) in rows.enumerate() {
        if index > 0 {
            output.write_all(b",").map_err(|_| Error::File)?;
        }
        output
            .write_all(row.map_err(|_| Error::Storage)?.as_bytes())
            .map_err(|_| Error::File)?;
    }
    Ok(())
}

fn job_from_row(row: &Row<'_>) -> rusqlite::Result<Job> {
    let id: String = row.get(0)?;
    let config: String = row.get(2)?;
    let decode = |index, error| {
        rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, error)
    };
    Ok(Job {
        id: id.parse().map_err(|e| decode(0, Box::new(e)))?,
        status: row.get(1)?,
        config: serde_json::from_str(&config).map_err(|e| decode(2, Box::new(e)))?,
        created_at: row.get(3)?,
        finished_at: row.get(4)?,
        pages_crawled: row.get(5)?,
        assets_downloaded: row.get(6)?,
    })
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
