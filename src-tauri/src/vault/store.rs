//! Owns the versioned Vault format and the filesystem transactions around it.
use std::{
    fs::{self, File, Metadata, OpenOptions},
    io::{self, BufReader, Read, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

use age::secrecy::{ExposeSecret, SecretString};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroizing;

const KEY: &str = "wits-vault-v1.age";
const CATALOG: &str = "catalog.age";
const OBJECTS: &str = "objects";
const LOCK: &str = "vault.lock";
const LABEL: &str = "WITS Tools Vault v1 catalog authentication";

// Keep all original names and directory structure inside the authenticated catalog.
#[derive(Clone, Deserialize, Serialize)]
pub(super) struct Entry {
    pub id: Uuid,
    pub name: String,
    pub folder: bool,
    files: Vec<Node>,
}

#[derive(Clone, Deserialize, Serialize)]
struct Node {
    path: Vec<String>,
    object: Option<Uuid>,
    hash: Option<blake3::Hash>,
}

#[derive(Deserialize, Serialize)]
struct Catalog {
    vault: Uuid,
    entries: Vec<Entry>,
}

pub(super) struct Vault {
    pub root: PathBuf,
    identity: age::x25519::Identity,
    catalog: Catalog,
    _lock: File,
}

// Treat junctions and other reparse points like symlinks, even when Rust classifies them as directories.
fn plain(path: &Path) -> Result<Metadata> {
    let info = fs::symlink_metadata(path)?;
    ensure!(
        !info.is_symlink(),
        "Links are not supported: {}",
        path.display()
    );
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        ensure!(
            info.file_attributes() & 0x400 == 0,
            "Reparse points are not supported: {}",
            path.display()
        );
    }
    ensure!(
        info.is_file() || info.is_dir(),
        "Unsupported file: {}",
        path.display()
    );
    Ok(info)
}

// Only use ordinary single path components from the catalog or native picker.
fn name(s: &str) -> Result<&str> {
    let mut parts = Path::new(s).components();
    ensure!(
        matches!(parts.next(), Some(Component::Normal(part)) if part == s)
            && parts.next().is_none()
            && !s.contains(['/', '\\', ':', '\0'])
            && !s.ends_with([' ', '.']),
        "Invalid file name: {s}"
    );
    Ok(s)
}

fn child_path(parent: &Path, parts: &[String]) -> Result<PathBuf> {
    let mut path = parent.to_owned();
    for part in parts {
        path.push(name(part)?);
    }
    Ok(path)
}

// Hold an exclusive lock while an unlocked session can change its catalog.
fn open_lock(root: &Path, create: bool) -> Result<File> {
    let path = root.join(LOCK);
    let file = if create {
        OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)?
    } else {
        ensure!(plain(&path)?.is_file(), "Invalid Vault lock file.");
        OpenOptions::new().read(true).write(true).open(path)?
    };
    fs2::FileExt::try_lock_exclusive(&file).context("This Vault is open in another window.")?;
    Ok(file)
}

fn reader(path: &Path) -> Result<BufReader<File>> {
    ensure!(
        plain(path)?.is_file(),
        "Expected a file: {}",
        path.display()
    );
    Ok(BufReader::new(File::open(path)?))
}

fn cipher(identity: &age::x25519::Identity) -> Result<age::Encryptor> {
    let public = identity.to_public();
    Ok(age::Encryptor::with_recipients(
        [&public as &dyn age::Recipient].into_iter(),
    )?)
}

// Stage each change on the same volume and flush data before publishing it.
fn atomic(path: &Path, replace: bool, write: impl FnOnce(&mut File) -> Result<()>) -> Result<()> {
    let mut temp =
        tempfile::NamedTempFile::new_in(path.parent().context("Missing destination folder.")?)?;
    write(temp.as_file_mut())?;
    temp.as_file().sync_all()?;
    let stage = temp.into_temp_path();
    commit(&stage, path, replace)
}

fn checkpoint(cancel: &AtomicBool) -> Result<()> {
    ensure!(!cancel.load(Ordering::Relaxed), "Transfer cancelled.");
    Ok(())
}

// Hash ciphertext as it passes through the stream so the catalog can bind an ID to the exact object.
fn transfer(input: &mut impl Read, output: &mut impl Write, cancel: &AtomicBool) -> Result<()> {
    let mut buffer = Zeroizing::new(vec![0u8; 128 * 1024]);
    loop {
        checkpoint(cancel)?;
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        output.write_all(&buffer[..count])?;
    }
    Ok(())
}

// Authenticate the catalog separately from public-recipient encryption.
fn catalog_tag(identity: &age::x25519::Identity, ciphertext: &[u8]) -> blake3::Hash {
    let key = Zeroizing::new(blake3::derive_key(
        LABEL,
        identity.to_string().expose_secret().as_bytes(),
    ));
    blake3::keyed_hash(&key, ciphertext)
}

fn save_catalog(
    root: &Path,
    identity: &age::x25519::Identity,
    catalog: &Catalog,
    replace: bool,
) -> Result<()> {
    let mut ciphertext = Vec::new();
    let mut stream = cipher(identity)?.wrap_output(&mut ciphertext)?;
    serde_json::to_writer(&mut stream, catalog)?;
    stream.finish()?;
    atomic(&root.join(CATALOG), replace, |out| {
        out.write_all(catalog_tag(identity, &ciphertext).as_bytes())?;
        out.write_all(&ciphertext)?;
        Ok(())
    })
}

// Inspect only the known Vault structure and show its location before any destructive action.
pub(super) fn inspect(root: &Path) -> Result<PathBuf> {
    ensure!(plain(root)?.is_dir(), "Choose the Vault folder.");
    let root = root.canonicalize()?;
    for item in [KEY, CATALOG, LOCK] {
        ensure!(
            plain(&root.join(item))?.is_file(),
            "This folder does not contain a WITS Vault."
        );
    }
    ensure!(
        plain(&root.join(OBJECTS))?.is_dir(),
        "Vault object folder is missing."
    );
    Ok(root)
}

impl Vault {
    // Build a complete empty Vault privately before putting it in the chosen location.
    pub fn create(parent: &Path, password: String) -> Result<Self> {
        ensure!(
            password.chars().count() >= 12,
            "Use a password of at least 12 characters."
        );
        let root = parent.join("WITS Vault");
        ensure!(!root.exists(), "A WITS Vault folder already exists here.");
        let stage = tempfile::tempdir_in(parent)?;
        let identity = age::x25519::Identity::generate();
        let catalog = Catalog {
            vault: Uuid::new_v4(),
            entries: Vec::new(),
        };
        fs::create_dir(stage.path().join(OBJECTS))?;
        drop(open_lock(stage.path(), true)?);
        atomic(&stage.path().join(KEY), false, |out| {
            let mut enc = age::Encryptor::with_user_passphrase(SecretString::from(password))
                .wrap_output(out)?;
            enc.write_all(identity.to_string().expose_secret().as_bytes())?;
            enc.finish()?;
            Ok(())
        })?;
        save_catalog(stage.path(), &identity, &catalog, false)?;
        commit(stage.path(), &root, false)?;
        let lock = open_lock(&root, false)?;
        Ok(Self {
            root: root.canonicalize()?,
            identity,
            catalog,
            _lock: lock,
        })
    }

    // The selected folder and password are sufficient, including after reinstall.
    pub fn open(root: &Path, password: String) -> Result<Self> {
        let root = inspect(root)?;
        let lock = open_lock(&root, false)?;
        let pass = age::scrypt::Identity::new(SecretString::from(password));
        let mut unlock = age::Decryptor::new_buffered(reader(&root.join(KEY))?)?
            .decrypt([&pass as &dyn age::Identity].into_iter())
            .map_err(|_| {
                anyhow::anyhow!("The password is incorrect or the Vault key is damaged.")
            })?;
        let mut secret = Zeroizing::new(String::new());
        (&mut unlock).take(256).read_to_string(&mut secret)?;
        let identity = secret
            .parse::<age::x25519::Identity>()
            .map_err(anyhow::Error::msg)?;
        let mut stored = Vec::new();
        reader(&root.join(CATALOG))?.read_to_end(&mut stored)?;
        let (tag, ciphertext) = stored
            .split_at_checked(32)
            .context("Vault catalog is incomplete.")?;
        let expected: [u8; 32] = tag.try_into()?;
        ensure!(
            blake3::Hash::from_bytes(expected) == catalog_tag(&identity, ciphertext),
            "Vault catalog failed authentication."
        );
        let contents = age::Decryptor::new_buffered(ciphertext)?
            .decrypt([&identity as &dyn age::Identity].into_iter())?;
        let catalog = serde_json::from_reader(contents)?;
        Ok(Self {
            root,
            identity,
            catalog,
            _lock: lock,
        })
    }

    pub fn entries(&self) -> &[Entry] {
        &self.catalog.entries
    }

    // Commit a complete selection before attempting any removal of original files.
    pub fn import(
        &mut self,
        source: &Path,
        remove: bool,
        cancel: &AtomicBool,
    ) -> Result<Vec<String>> {
        let info = plain(source)?;
        let source = source.canonicalize()?;
        ensure!(
            !source.starts_with(&self.root) && !self.root.starts_with(&source),
            "Choose an item outside the Vault folder and its parents."
        );
        let label = name(
            source
                .file_name()
                .and_then(|s| s.to_str())
                .context("Unsupported file name.")?,
        )?
        .to_owned();
        let mut entry = Entry {
            id: Uuid::new_v4(),
            name: label,
            folder: info.is_dir(),
            files: Vec::new(),
        };
        let mut written = Vec::new();
        let mut originals = Vec::new();
        let result = (|| {
            self.import_node(
                &source,
                Vec::new(),
                &mut entry.files,
                &mut written,
                &mut originals,
                cancel,
            )?;
            checkpoint(cancel)?;
            self.catalog.entries.push(entry);
            if let Err(error) = save_catalog(&self.root, &self.identity, &self.catalog, true) {
                self.catalog.entries.pop();
                return Err(error);
            }
            Ok(())
        })();
        if let Err(error) = result {
            for object in written {
                let _ = fs::remove_file(object);
            }
            return Err(error);
        }
        Ok(if remove {
            remove_originals(originals, cancel)
        } else {
            Vec::new()
        })
    }

    // Walk directories without following links. Each path is relative to the selected item.
    fn import_node(
        &self,
        path: &Path,
        relative: Vec<String>,
        nodes: &mut Vec<Node>,
        written: &mut Vec<PathBuf>,
        originals: &mut Vec<(PathBuf, Option<blake3::Hash>)>,
        cancel: &AtomicBool,
    ) -> Result<()> {
        checkpoint(cancel)?;
        let info = plain(path)?;
        if info.is_dir() {
            nodes.push(Node {
                path: relative.clone(),
                object: None,
                hash: None,
            });
            for child in fs::read_dir(path)? {
                let path = child?.path();
                let mut parts = relative.clone();
                parts.push(
                    name(
                        path.file_name()
                            .and_then(|part| part.to_str())
                            .context("Unsupported file name.")?,
                    )?
                    .to_owned(),
                );
                self.import_node(&path, parts, nodes, written, originals, cancel)?;
            }
            originals.push((path.to_path_buf(), None));
        } else {
            let id = Uuid::new_v4();
            let object = self.root.join(OBJECTS).join(id.to_string());
            let mut source = blake3::Hasher::new();
            let mut hash = blake3::Hasher::new();
            atomic(&object, false, |out| {
                let mut encrypted = cipher(&self.identity)?
                    .wrap_output(io::BufWriter::new(HashWriter(out, &mut hash)))?;
                let mut input = HashReader(reader(path)?, &mut source);
                transfer(&mut input, &mut encrypted, cancel)?;
                encrypted.finish()?.flush()?;
                Ok(())
            })?;
            written.push(object);
            originals.push((path.to_path_buf(), Some(source.finalize())));
            nodes.push(Node {
                path: relative,
                object: Some(id),
                hash: Some(hash.finalize()),
            });
        }
        Ok(())
    }

    // Validate each object before publishing the requested plaintext output.
    pub fn export(&self, id: Option<Uuid>, parent: &Path, cancel: &AtomicBool) -> Result<PathBuf> {
        let parent = parent.canonicalize()?;
        ensure!(
            !parent.starts_with(&self.root),
            "Choose a destination outside the Vault."
        );
        ensure!(plain(&parent)?.is_dir(), "Choose a destination folder.");
        let temp = tempfile::Builder::new()
            .prefix(".wits-vault-export-")
            .tempdir_in(&parent)?;
        let result = if let Some(id) = id {
            let entry = self
                .catalog
                .entries
                .iter()
                .find(|entry| entry.id == id)
                .context("Vault item is missing.")?;
            let dest = parent.join(name(&entry.name)?);
            ensure!(
                !dest.exists(),
                "An item with this name already exists. Choose another folder."
            );
            let stage = temp.path().join("item");
            self.restore_entry(entry, &stage, cancel)?;
            commit(&stage, &dest, false)?;
            dest
        } else {
            let dest = parent.join("Decrypted WITS Vault");
            ensure!(
                !dest.exists(),
                "Decrypted WITS Vault already exists. Choose another folder."
            );
            let stage = temp.path().join("all");
            fs::create_dir(&stage)?;
            for entry in &self.catalog.entries {
                let mut path = stage.join(name(&entry.name)?);
                if path.exists() {
                    path = stage.join(format!("{}-{}", entry.id, entry.name));
                }
                self.restore_entry(entry, &path, cancel)?;
            }
            commit(&stage, &dest, false)?;
            dest
        };
        Ok(result)
    }

    fn restore_entry(&self, entry: &Entry, output: &Path, cancel: &AtomicBool) -> Result<()> {
        for node in &entry.files {
            checkpoint(cancel)?;
            let path = child_path(output, &node.path)?;
            if let Some(id) = node.object {
                let mut hash = blake3::Hasher::new();
                let input = BufReader::new(HashReader(
                    reader(&self.root.join(OBJECTS).join(id.to_string()))?,
                    &mut hash,
                ));
                let mut data = age::Decryptor::new_buffered(input)?
                    .decrypt([&self.identity as &dyn age::Identity].into_iter())?;
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)?;
                transfer(&mut data, &mut file, cancel)?;
                drop(data);
                ensure!(
                    Some(hash.finalize()) == node.hash,
                    "A stored file failed authentication."
                );
                file.sync_all()?;
            } else {
                fs::create_dir(path)?;
            }
        }
        Ok(())
    }
}

// Request a durable same-volume rename and preserve existing destinations unless replacing the catalog.
fn commit(from: &Path, to: &Path, replace: bool) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        };
        let from: Vec<_> = from.as_os_str().encode_wide().chain([0]).collect();
        let to: Vec<_> = to.as_os_str().encode_wide().chain([0]).collect();
        let flags = MOVEFILE_WRITE_THROUGH
            | if replace {
                MOVEFILE_REPLACE_EXISTING
            } else {
                0
            };
        if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), flags) } == 0 {
            return Err(io::Error::last_os_error().into());
        }
    }
    #[cfg(not(windows))]
    {
        ensure!(replace || !to.exists(), "The destination already exists.");
        fs::rename(from, to)?;
    }
    Ok(())
}
// Delete only unchanged regular files. Empty-directory removal preserves new files.
fn remove_originals(
    entries: Vec<(PathBuf, Option<blake3::Hash>)>,
    cancel: &AtomicBool,
) -> Vec<String> {
    let mut left = Vec::new();
    for (path, hash) in entries {
        let result = (|| {
            checkpoint(cancel)?;
            plain(&path)?;
            if let Some(expected) = hash {
                remove_unchanged_file(&path, expected)?;
            } else {
                fs::remove_dir(&path)?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            let error: anyhow::Error = error;
            left.push(format!("{} ({error})", path.display()));
        }
    }
    left
}

// Keep the verified file handle open through deletion to prevent replacement of the final path.
fn remove_unchanged_file(path: &Path, expected: blake3::Hash) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::{
            fs::{MetadataExt, OpenOptionsExt},
            io::AsRawHandle,
        };
        use windows_sys::Win32::{
            Foundation::GENERIC_READ,
            Storage::FileSystem::{
                DELETE, FILE_DISPOSITION_INFO, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
                FileDispositionInfo, SetFileInformationByHandle,
            },
        };
        let mut file = OpenOptions::new()
            .access_mode(GENERIC_READ | DELETE)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        let info = file.metadata()?;
        ensure!(
            info.is_file() && info.file_attributes() & 0x400 == 0,
            "The original changed during import."
        );
        let mut current = blake3::Hasher::new();
        io::copy(&mut file, &mut current)?;
        ensure!(
            expected == current.finalize(),
            "The original changed during import."
        );
        let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
        if unsafe {
            SetFileInformationByHandle(
                file.as_raw_handle(),
                FileDispositionInfo,
                (&disposition as *const FILE_DISPOSITION_INFO).cast(),
                size_of::<FILE_DISPOSITION_INFO>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error().into());
        }
    }
    #[cfg(not(windows))]
    {
        let mut current = blake3::Hasher::new();
        io::copy(&mut reader(path)?, &mut current)?;
        ensure!(
            expected == current.finalize(),
            "The original changed during import."
        );
        fs::remove_file(path)?;
    }
    Ok(())
}
// With no password, validate the dedicated layout and remove only files owned by Vault.
pub(super) fn delete(root: &Path) -> Result<()> {
    let root = inspect(root)?;
    let lock = open_lock(&root, false)?;
    let objects = root.join(OBJECTS);
    let files = fs::read_dir(&objects)?
        .map(|entry| -> Result<PathBuf> {
            let path = entry?.path();
            ensure!(
                plain(&path)?.is_file()
                    && path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .is_some_and(|s| Uuid::parse_str(s).is_ok()),
                "Vault contains unexpected files. Nothing was deleted."
            );
            Ok(path)
        })
        .collect::<Result<Vec<_>>>()?;
    for file in fs::read_dir(&root)? {
        let file = file?;
        ensure!(
            [KEY, CATALOG, LOCK, OBJECTS]
                .iter()
                .any(|name| file.file_name() == *name),
            "Vault contains unexpected files. Nothing was deleted."
        );
    }
    ensure!(
        reader(&root.join(KEY))?.fill_prefix_is_age()?,
        "Invalid Vault key file."
    );
    for path in files {
        fs::remove_file(path)?;
    }
    fs::remove_dir(objects)?;
    fs::remove_file(root.join(CATALOG))?;
    fs::remove_file(root.join(KEY))?;
    drop(lock);
    fs::remove_file(root.join(LOCK))?;
    fs::remove_dir(root)?;
    Ok(())
}

trait AgePrefix {
    fn fill_prefix_is_age(&mut self) -> io::Result<bool>;
}
impl<T: Read> AgePrefix for T {
    fn fill_prefix_is_age(&mut self) -> io::Result<bool> {
        let mut prefix = [0; 22];
        self.read_exact(&mut prefix)?;
        Ok(&prefix == b"age-encryption.org/v1\n")
    }
}

// Stream adapters keep hashing behavior independent of the transfer and encryption loops.
struct HashReader<'a, T>(T, &'a mut blake3::Hasher);
impl<T: Read> Read for HashReader<'_, T> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let count = self.0.read(buf)?;
        self.1.update(&buf[..count]);
        Ok(count)
    }
}

struct HashWriter<'a, T>(T, &'a mut blake3::Hasher);
impl<T: Write> Write for HashWriter<'_, T> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let count = self.0.write(buf)?;
        self.1.update(&buf[..count]);
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

// Exercise the persisted data and actual filesystem boundaries used by the desktop commands.
#[cfg(test)]
mod tests {
    use super::*;

    const PASSWORD: &str = "example recovery passphrase";

    #[test]
    fn moved_folder_reopens_elsewhere_and_preserves_structure() -> Result<()> {
        let work = tempfile::tempdir()?;
        let source = work.path().join("Original");
        fs::create_dir_all(source.join("Nested").join("Empty"))?;
        let contents = vec![0x97; 300_000];
        fs::write(source.join("Nested").join("Media.bin"), &contents)?;
        let mut vault = Vault::create(work.path(), PASSWORD.into())?;
        assert!(Vault::open(&vault.root, PASSWORD.into()).is_err());
        let cancel = AtomicBool::new(false);
        assert!(vault.import(&source, true, &cancel)?.is_empty());
        assert!(!source.exists());
        let root = vault.root.clone();
        drop(vault);

        let relocated = work.path().join("Relocated");
        fs::rename(root, &relocated)?;
        assert!(Vault::open(&relocated, "incorrect password".into()).is_err());
        let vault = Vault::open(&relocated, PASSWORD.into())?;
        let id = vault.entries()[0].id;
        let destination = work.path().join("Recovered");
        fs::create_dir(&destination)?;
        let restored = vault.export(Some(id), &destination, &cancel)?;
        assert_eq!(
            fs::read(restored.join("Nested").join("Media.bin"))?,
            contents
        );
        assert!(restored.join("Nested").join("Empty").is_dir());
        assert!(vault.export(Some(id), &destination, &cancel).is_err());
        Ok(())
    }

    #[test]
    fn interrupted_and_corrupt_transfers_keep_the_existing_files() -> Result<()> {
        let work = tempfile::tempdir()?;
        let file = work.path().join("Original");
        fs::write(&file, vec![0x45; 100_000])?;
        let mut vault = Vault::create(work.path(), PASSWORD.into())?;
        let cancel = AtomicBool::new(true);
        assert!(vault.import(&file, true, &cancel).is_err());
        assert!(file.exists());
        assert!(vault.entries().is_empty());

        cancel.store(false, Ordering::Relaxed);
        vault.import(&file, false, &cancel)?;
        let object = vault.entries()[0].files[0]
            .object
            .context("Missing object.")?;
        let path = vault.root.join(OBJECTS).join(object.to_string());
        OpenOptions::new().write(true).open(path)?.set_len(400)?;
        assert!(vault.export(None, work.path(), &cancel).is_err());
        assert!(!work.path().join("Decrypted WITS Vault").exists());
        assert!(file.exists());
        Ok(())
    }

    #[test]
    fn removal_rejects_unrelated_files_and_accepts_a_verified_vault() -> Result<()> {
        let work = tempfile::tempdir()?;
        let vault = Vault::create(work.path(), PASSWORD.into())?;
        let root = vault.root.clone();
        drop(vault);
        let unrelated = root.join("Keep.txt");
        fs::write(&unrelated, "keep")?;
        assert!(delete(&root).is_err());
        assert!(root.join(KEY).exists());
        assert!(unrelated.exists());
        fs::remove_file(unrelated)?;
        delete(&root)?;
        assert!(!root.exists());
        Ok(())
    }
}
