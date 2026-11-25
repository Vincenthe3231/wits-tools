//! Reserves output names and publishes completed files on the destination volume.

use std::{
    fs::{File, OpenOptions},
    io::ErrorKind,
    path::Path,
};

use tempfile::TempPath;

// Distinguish a name conflict from other output failures for caller recovery.
pub(crate) enum Error {
    Conflict,
    File,
}

// The reservation also coordinates writers across the desktop and CLI processes.
pub(crate) fn reserve(output: &Path, overwrite: bool) -> Result<File, Error> {
    let folder = output.parent().ok_or(Error::File)?;
    // Resolve directory aliases so desktop and CLI writers use the same reservation.
    let folder = std::fs::canonicalize(folder).map_err(|_| Error::File)?;
    let reservations = crate::crawler::data_dir()
        .map_err(|_| Error::File)?
        .with_file_name("download-locks");
    std::fs::create_dir_all(&reservations).map_err(|_| Error::File)?;
    let name = output
        .file_name()
        .ok_or(Error::File)?
        .to_string_lossy()
        .to_lowercase();
    let digest = blake3::hash(
        folder
            .join(name)
            .to_string_lossy()
            .to_lowercase()
            .as_bytes(),
    );
    let reservation = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(reservations.join(format!("{digest}.lock")))
        .map_err(|_| Error::File)?;
    reservation.try_lock().map_err(|_| Error::Conflict)?;
    if output.exists() && !overwrite {
        return Err(Error::Conflict);
    }
    Ok(reservation)
}

// Stage on the destination volume so publication does not require copying the file.
pub(crate) fn stage(folder: &Path) -> Result<TempPath, Error> {
    Ok(tempfile::NamedTempFile::new_in(folder)
        .map_err(|_| Error::File)?
        .into_temp_path())
}

// Check for a destination created by a different app after the reservation.
pub(crate) fn publish(staged: TempPath, output: &Path, overwrite: bool) -> Result<(), Error> {
    if overwrite {
        staged.persist(output).map_err(|_| Error::File)
    } else {
        staged.persist_noclobber(output).map_err(|error| {
            if error.error.kind() == ErrorKind::AlreadyExists {
                Error::Conflict
            } else {
                Error::File
            }
        })
    }
}
