//! Exposes crawl and media operations to scripts with normal console output and exit codes.

use std::{path::PathBuf, sync::Arc};

use clap::{Args, Parser, Subcommand, ValueEnum};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use wits_tools_lib::crawler::{
    self, Error, Result,
    crawl::{self, Config, Mode},
    media::{self, Download},
    storage::Store,
};

#[derive(Parser)]
#[command(version, about = "Crawl sites and download media from WITS Tools")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Crawl one or more seed URLs and store the results.
    Crawl {
        #[arg(required = true)]
        urls: Vec<String>,
        #[arg(long, value_enum, default_value = "auto")]
        mode: CrawlMode,
        #[arg(long, default_value_t = 100)]
        max_pages: u32,
        #[arg(long, default_value_t = 5)]
        max_depth: u32,
        #[arg(long, default_value_t = 2)]
        concurrency: usize,
        #[arg(long, default_value_t = 500)]
        delay: u64,
        #[arg(long)]
        proxy: Option<String>,
        #[arg(long)]
        no_assets: bool,
        #[arg(long)]
        allow_domain: Vec<String>,
        #[arg(long)]
        exclude: Vec<String>,
        #[arg(long)]
        ignore_robots: bool,
    },
    /// Export a persisted crawl and all result rows to JSON.
    Export {
        id: Uuid,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Save a direct file, HLS playlist, or DASH video representation.
    Download {
        #[command(flatten)]
        transfer: TransferArgs,
    },
    /// Inspect a media URL and print its available formats.
    Info {
        #[command(flatten)]
        transfer: TransferArgs,
    },
    /// Run the opt-in local HTTP capture proxy.
    Proxy {
        #[command(subcommand)]
        operation: ProxyCommand,
    },
    /// Run the opt-in local HTTP capture proxy.
    Sniff {
        #[command(flatten)]
        options: ProxyOptions,
    },
    /// Register the optional browser native messaging host for the current Windows user.
    CaptureRegister,
}

#[derive(Subcommand)]
enum ProxyCommand {
    Run {
        #[command(flatten)]
        options: ProxyOptions,
    },
}

#[derive(Args)]
struct ProxyOptions {
    #[arg(short, long, default_value_t = 9000)]
    port: u16,
    #[arg(long)]
    auto: bool,
    #[arg(short, long, default_value = "download")]
    output: PathBuf,
}

#[derive(Args)]
struct TransferArgs {
    url: String,
    #[arg(short, long)]
    output: Option<PathBuf>,
    #[arg(long)]
    filename: Option<String>,
    #[arg(short, long)]
    format: Option<String>,
    #[arg(short = 'x', long)]
    proxy: Option<String>,
    #[arg(short, long)]
    workers: Option<usize>,
    #[arg(long)]
    force: bool,
    #[arg(long, value_parser = parse_header)]
    header: Vec<(String, String)>,
    #[arg(long)]
    cookie: Option<String>,
    #[arg(long)]
    cookie_file: Option<PathBuf>,
    #[arg(long, value_enum)]
    cookies_from_browser: Option<CookieBrowser>,
    #[arg(short = 'U', long)]
    user_agent: Option<String>,
}

#[derive(Clone, ValueEnum)]
enum CrawlMode {
    Auto,
    Browser,
    Static,
}

#[derive(Clone, Copy, ValueEnum)]
enum CookieBrowser {
    Chrome,
    Edge,
}

fn parse_header(value: &str) -> std::result::Result<(String, String), String> {
    let (name, value) = value
        .split_once(':')
        .ok_or("Use NAME: VALUE for a header")?;
    Ok((name.trim().into(), value.trim().into()))
}

impl TransferArgs {
    fn request(self) -> Result<Download> {
        let mut headers = self.header;
        if let Some(agent) = self.user_agent {
            headers.push(("user-agent".into(), agent));
        }
        let cookie = if let Some(cookie) = self.cookie {
            Some(cookie)
        } else if let Some(file) = self.cookie_file {
            Some(cookie_file(&file, &self.url)?)
        } else if let Some(browser) = self.cookies_from_browser {
            match browser_cookies(browser, &self.url) {
                Ok(cookie) => Some(cookie),
                Err(_) => {
                    eprintln!("No readable browser cookies were available for this URL.");
                    None
                }
            }
        } else {
            None
        };
        if let Some(cookie) = cookie.filter(|cookie| !cookie.is_empty()) {
            headers.push(("cookie".into(), cookie));
        }
        Ok(Download {
            url: self.url,
            folder: self.output,
            filename: self.filename,
            format: self.format,
            proxy: self.proxy,
            workers: self.workers,
            overwrite: self.force,
            headers,
        })
    }
}

/// Match the legacy opt-in behavior for plaintext cookies scoped to the requested URL.
fn browser_cookies(browser: CookieBrowser, url: &str) -> Result<String> {
    let url = crawler::http_url(url)?;
    let host = url.host_str().ok_or(Error::InvalidUrl)?;
    let local = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or(Error::File)?;
    let profile = local.join(match browser {
        CookieBrowser::Chrome => "Google/Chrome/User Data/Default",
        CookieBrowser::Edge => "Microsoft/Edge/User Data/Default",
    });
    let db = [profile.join("Network/Cookies"), profile.join("Cookies")]
        .into_iter()
        .find_map(|path| {
            rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .ok()
        })
        .ok_or(Error::File)?;
    let mut statement = db
        .prepare(
            "SELECT host_key,path,is_secure,name,value FROM cookies
         WHERE value<>'' AND (expires_utc=0 OR expires_utc>?1) ORDER BY length(path) DESC",
        )
        .map_err(|_| Error::Storage)?;
    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let chromium_time = (epoch as i64 + 11_644_473_600) * 1_000_000;
    let rows = statement
        .query_map([chromium_time], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, bool>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(|_| Error::Storage)?;
    let mut cookies = Vec::new();
    for row in rows {
        let (domain, path, secure, name, value) = row.map_err(|_| Error::Storage)?;
        let valid_domain = if let Some(suffix) = domain.strip_prefix('.') {
            host == suffix || host.ends_with(&domain)
        } else {
            host == domain
        };
        let valid_path = url.path() == path
            || url
                .path()
                .strip_prefix(&path)
                .is_some_and(|tail| path.ends_with('/') || tail.starts_with('/'));
        if valid_domain && valid_path && (!secure || url.scheme() == "https") {
            cookies.push(format!("{name}={value}"));
        }
    }
    Ok(cookies.join("; "))
}

/// Read only cookies that belong to this host, path, and scheme from an explicit Netscape file.
fn cookie_file(file: &std::path::Path, url: &str) -> Result<String> {
    let url = crawler::http_url(url)?;
    let host = url.host_str().ok_or(Error::InvalidUrl)?;
    let contents = std::fs::read_to_string(file).map_err(|_| Error::File)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let cookies = contents
        .lines()
        .filter_map(|line| {
            let line = line.strip_prefix("#HttpOnly_").unwrap_or(line);
            if line.starts_with('#') {
                return None;
            }
            let fields: Vec<_> = line.splitn(7, '\t').collect();
            let [domain, inclusive, path, secure, expires, name, value] = fields.as_slice() else {
                return None;
            };
            let domain = domain.trim_start_matches('.');
            let valid_domain = host.eq_ignore_ascii_case(domain)
                || *inclusive == "TRUE"
                    && host
                        .strip_suffix(domain)
                        .is_some_and(|prefix| prefix.ends_with('.'));
            let valid_path = url.path() == *path
                || url
                    .path()
                    .strip_prefix(*path)
                    .is_some_and(|tail| path.ends_with('/') || tail.starts_with('/'));
            let expiry = expires.parse::<u64>().ok()?;
            (valid_domain
                && valid_path
                && (*secure != "TRUE" || url.scheme() == "https")
                && (expiry == 0 || expiry > now))
                .then(|| format!("{name}={value}"))
        })
        .collect::<Vec<_>>();
    Ok(cookies.join("; "))
}

/// Share cancellation with all work started by this console invocation.
#[tokio::main]
async fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    let stop = CancellationToken::new();
    let signal = stop.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            signal.cancel();
        }
    });
    match run(cli.command, stop).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run(command: Command, stop: CancellationToken) -> Result<()> {
    match command {
        Command::Crawl {
            urls,
            mode,
            max_pages,
            max_depth,
            concurrency,
            delay,
            proxy,
            no_assets,
            allow_domain,
            exclude,
            ignore_robots,
        } => {
            let mode = match mode {
                CrawlMode::Auto => Mode::Auto,
                CrawlMode::Browser => Mode::Browser,
                CrawlMode::Static => Mode::Static,
            };
            let config = Config {
                start_urls: urls,
                mode,
                max_pages,
                max_depth,
                concurrency,
                request_delay_millis: delay,
                download_assets: !no_assets,
                allowed_domains: allow_domain,
                exclude_patterns: exclude,
                respect_robots_txt: !ignore_robots,
                proxy_url: proxy,
            };
            config.validate()?;
            let store = Store::open(crawler::data_dir()?)?;
            let id = Uuid::new_v4();
            let lease = store.create(id, &config)?;
            println!("{id}");
            crawl::run(store, id, config, crawler::browser_path(), lease, stop).await?;
        }
        Command::Export { id, output } => {
            let path = output.unwrap_or_else(|| PathBuf::from(format!("crawl-{id}.json")));
            Store::open(crawler::data_dir()?)?.export(id, &path)?;
            println!("{}", path.display());
        }
        Command::Download { transfer } => {
            let path = media::download(
                transfer.request()?,
                None,
                PathBuf::from("download"),
                stop,
                |progress| {
                    eprint!("\r{} bytes", progress.bytes);
                },
            )
            .await?;
            eprintln!();
            println!("{}", path.display());
        }
        Command::Info { transfer } => {
            let info = media::info(&transfer.request()?, None).await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&info).map_err(|_| Error::InvalidManifest)?
            );
        }
        Command::Proxy {
            operation: ProxyCommand::Run { options },
        }
        | Command::Sniff { options } => {
            eprintln!("Listening on 127.0.0.1:{}", options.port);
            let transfer_stop = stop.clone();
            let active = Arc::new(tokio::sync::Semaphore::new(4));
            crawler::proxy::run(options.port, stop, move |url, kind| {
                let mut display = crawler::http_url(&url).ok();
                if let Some(url) = &mut display {
                    url.set_query(None);
                }
                if let Some(display) = display {
                    println!("[{kind:?}] {display}");
                }
                if let Ok(permit) = active.clone().try_acquire_owned()
                    && options.auto
                {
                    let folder = options.output.clone();
                    let stop = transfer_stop.clone();
                    tokio::spawn(async move {
                        let _permit = permit;
                        let request = Download {
                            url,
                            ..Download::default()
                        };
                        match media::download(request, None, folder, stop, |_| {}).await {
                            Ok(path) => println!("{}", path.display()),
                            Err(error) => eprintln!("{error}"),
                        }
                    });
                }
            })
            .await?;
        }
        Command::CaptureRegister => crawler::capture::register()?,
    }
    Ok(())
}
