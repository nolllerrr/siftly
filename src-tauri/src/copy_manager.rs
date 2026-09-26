use crate::scanner::OperationError;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs::{self, File, FileTimes, Metadata, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::Instant,
};

const CHUNK_SIZE: usize = 1024 * 1024;

#[derive(Debug, Deserialize)]
pub struct SelectedFile {
    pub path: String,
    pub size: u64,
}

#[derive(Debug, Deserialize)]
pub struct CopyRequest {
    pub files: Vec<SelectedFile>,
    pub destination: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct CopyProgress {
    pub completed: usize,
    pub total: usize,
    // Committed files plus the current chunk; rolled back if the current file fails.
    pub bytes_copied: u64,
    pub total_bytes: u64,
    pub current_file: String,
}

#[derive(Debug, Serialize)]
pub struct CopyResult {
    #[serde(rename = "type")]
    pub event_type: &'static str,
    pub total: usize,
    pub copied: usize,
    pub failed: usize,
    pub skipped: usize,
    pub bytes_copied: u64,
    pub cancelled: bool,
    pub duration: f64,
    pub errors: Vec<OperationError>,
}

// Keep Windows sources stable while reading and prevent destination replacement
// while its handle is open. Sharing violations become ordinary per-file errors.
fn open_source(path: &Path) -> io::Result<File> {
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(io::Error::other("Symbolic links are not copied"));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
        };
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        options.share_mode(FILE_SHARE_READ);
    }
    let file = options.open(path)?;
    crate::file_safety::verify_open_file(&file, path)?;
    Ok(file)
}

fn create_destination(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            DELETE, FILE_GENERIC_WRITE, FILE_SHARE_READ,
        };
        options.access_mode(FILE_GENERIC_WRITE | DELETE);
        options.share_mode(FILE_SHARE_READ);
    }
    options.open(path)
}

fn reserve_destination(
    destination: &Path,
    filename: &str,
    names: &mut HashSet<String>,
    is_cancelled: &impl Fn() -> bool,
) -> io::Result<Option<(PathBuf, File)>> {
    let name_path = Path::new(filename);
    let stem = name_path.file_stem().unwrap_or_default().to_string_lossy();
    let extension = name_path
        .extension()
        .map(|value| format!(".{}", value.to_string_lossy()))
        .unwrap_or_default();
    for suffix in 0_u64..10_000 {
        if is_cancelled() {
            return Ok(None);
        }
        let name = if suffix == 0 {
            filename.to_string()
        } else {
            format!("{stem}_{suffix}{extension}")
        };
        if names.contains(&name.to_lowercase()) {
            continue;
        }
        let path = destination.join(&name);
        // create_new is the authoritative check, even if another operation has
        // added an entry since the directory snapshot was collected.
        match create_destination(&path) {
            Ok(file) => {
                names.insert(name.to_lowercase());
                return Ok(Some((path, file)));
            }
            Err(error)
                if error.kind() == io::ErrorKind::AlreadyExists
                    || fs::symlink_metadata(&path).is_ok() =>
            {
                // Existing entries need not be retained in memory.
            }
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other("Cannot allocate a destination filename"))
}

// Bounded reads also detect files truncated during copying. A final size/mtime
// check below rejects a changing source on platforms without Windows sharing.
fn transfer(
    source: &mut impl Read,
    destination: &mut impl Write,
    size: u64,
    buffer: &mut [u8],
    is_cancelled: &impl Fn() -> bool,
    mut on_chunk: impl FnMut(u64),
) -> io::Result<Option<u64>> {
    let mut copied = 0;
    while copied < size {
        if is_cancelled() {
            return Ok(None);
        }
        let count = (size - copied).min(buffer.len() as u64) as usize;
        source.read_exact(&mut buffer[..count])?;
        destination.write_all(&buffer[..count])?;
        copied += count as u64;
        on_chunk(copied);
    }
    if is_cancelled() {
        return Ok(None);
    }
    Ok(Some(copied))
}

#[cfg(windows)]
const ATTRIBUTE_MASK: u32 = {
    use windows_sys::Win32::Storage::FileSystem::*;
    FILE_ATTRIBUTE_READONLY
        | FILE_ATTRIBUTE_HIDDEN
        | FILE_ATTRIBUTE_SYSTEM
        | FILE_ATTRIBUTE_ARCHIVE
        | FILE_ATTRIBUTE_NOT_CONTENT_INDEXED
};

#[cfg(windows)]
fn set_attributes(file: &File, attributes: u32) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FileBasicInfo, SetFileInformationByHandle, FILE_ATTRIBUTE_NORMAL, FILE_BASIC_INFO,
    };
    let info = FILE_BASIC_INFO {
        CreationTime: 0,
        LastAccessTime: 0,
        LastWriteTime: 0,
        ChangeTime: 0,
        FileAttributes: if attributes == 0 {
            FILE_ATTRIBUTE_NORMAL
        } else {
            attributes
        },
    };
    // SAFETY: the live file handle and correctly sized FILE_BASIC_INFO are valid
    // for the duration of this synchronous call; zero times leave dates intact.
    let success = unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileBasicInfo,
            &info as *const _ as *const _,
            std::mem::size_of::<FILE_BASIC_INFO>() as u32,
        )
    };
    if success == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn preserve_metadata(destination: &File, metadata: &Metadata) -> io::Result<()> {
    let times = FileTimes::new()
        .set_accessed(metadata.accessed()?)
        .set_modified(metadata.modified()?);
    #[cfg(windows)]
    let times = {
        use std::os::windows::fs::FileTimesExt;
        times.set_created(metadata.created()?)
    };
    destination.set_times(times)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        set_attributes(destination, metadata.file_attributes() & ATTRIBUTE_MASK)?;
    }
    #[cfg(not(windows))]
    destination.set_permissions(metadata.permissions())?;
    Ok(())
}

fn copy_one(
    source: &mut File,
    destination: &mut File,
    metadata: &Metadata,
    buffer: &mut [u8],
    is_cancelled: &impl Fn() -> bool,
    on_chunk: impl FnMut(u64),
) -> io::Result<Option<u64>> {
    #[cfg(windows)]
    crate::file_safety::copy_zone_identifier(source, destination)
        .map_err(|error| io::Error::other(format!("Cannot preserve Zone.Identifier: {error}")))?;
    let Some(copied) = transfer(
        source,
        destination,
        metadata.len(),
        buffer,
        is_cancelled,
        on_chunk,
    )?
    else {
        return Ok(None);
    };
    let after = source.metadata()?;
    if after.len() != metadata.len() || after.modified()? != metadata.modified()? {
        return Err(io::Error::other(
            "Source changed during copying; copy discarded",
        ));
    }
    destination.sync_all()?;
    if is_cancelled() {
        return Ok(None);
    }
    preserve_metadata(destination, metadata)?;
    Ok(Some(copied))
}

pub fn copy_files(
    request: CopyRequest,
    is_cancelled: impl Fn() -> bool,
    mut on_progress: impl FnMut(CopyProgress),
) -> Result<CopyResult, String> {
    let started = Instant::now();
    if request.files.len() > crate::security::MAX_FILES {
        return Err("Too many files in one copy operation".into());
    }
    let mut result = CopyResult {
        event_type: "copy_result",
        total: request.files.len(),
        copied: 0,
        failed: 0,
        skipped: 0,
        bytes_copied: 0,
        cancelled: false,
        duration: 0.0,
        errors: Vec::new(),
    };
    if is_cancelled() || request.files.is_empty() {
        result.cancelled = is_cancelled();
        result.skipped = result.total;
        return Ok(result);
    }
    if request.destination.trim().is_empty() {
        return Err("Choose a destination folder".into());
    }
    let destination = Path::new(&request.destination);
    fs::create_dir_all(destination)
        .map_err(|error| format!("Cannot open destination folder: {error}"))?;
    let _destination_locks =
        crate::file_safety::lock_directory(destination).map_err(|e| e.to_string())?;
    // create_new handles existing entries; don't load an unbounded directory listing.
    let mut names = HashSet::new();
    let mut total_bytes = request
        .files
        .iter()
        .fold(0_u64, |sum, item| sum.saturating_add(item.size));
    let mut buffer = vec![0; CHUNK_SIZE];

    for item in request.files {
        if is_cancelled() {
            result.cancelled = true;
            break;
        }
        let source_path = Path::new(&item.path);
        let filename = source_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let mut progress = CopyProgress {
            completed: result.copied + result.failed,
            total: result.total,
            bytes_copied: result.bytes_copied,
            total_bytes,
            current_file: filename.clone(),
        };
        on_progress(progress.clone());
        let opened = crate::file_safety::lock_directory(
            source_path.parent().unwrap_or_else(|| Path::new(".")),
        )
        .and_then(|locks| open_source(source_path).map(|file| (file, locks)))
        .and_then(|(file, locks)| {
            let metadata = file.metadata()?;
            if !metadata.is_file() {
                return Err(io::Error::other("Source is not a regular file"));
            }
            Ok((file, metadata, locks))
        });
        let outcome = match opened {
            Err(error) => Err(error),
            Ok((mut source, metadata, _source_locks)) => {
                total_bytes = total_bytes
                    .saturating_sub(item.size)
                    .saturating_add(metadata.len());
                progress.total_bytes = total_bytes;
                match reserve_destination(destination, &filename, &mut names, &is_cancelled) {
                    Err(error) => Err(error),
                    Ok(None) => Ok(None),
                    Ok(Some((path, mut output))) => {
                        let outcome = copy_one(
                            &mut source,
                            &mut output,
                            &metadata,
                            &mut buffer,
                            &is_cancelled,
                            |bytes| {
                                progress.bytes_copied = result.bytes_copied.saturating_add(bytes);
                                on_progress(progress.clone());
                            },
                        );
                        if !matches!(outcome, Ok(Some(_))) {
                            #[cfg(windows)]
                            let _ = set_attributes(&output, 0);
                            if let Err(error) = crate::file_safety::discard_output(&output, &path) {
                                result.errors.push(OperationError {
                                    path: path.to_string_lossy().into_owned(),
                                    message: format!(
                                        "Incomplete copy could not be removed: {error}"
                                    ),
                                });
                            }
                        }
                        drop(output);
                        outcome
                    }
                }
            }
        };
        match outcome {
            Ok(Some(bytes)) => {
                result.copied += 1;
                result.bytes_copied = result.bytes_copied.saturating_add(bytes);
            }
            Ok(None) => result.cancelled = true,
            Err(error) => {
                result.failed += 1;
                result.errors.push(OperationError {
                    path: item.path,
                    message: error.to_string(),
                });
            }
        }
        on_progress(CopyProgress {
            completed: result.copied + result.failed,
            total: result.total,
            bytes_copied: result.bytes_copied,
            total_bytes,
            current_file: filename,
        });
        if result.cancelled {
            break;
        }
    }
    result.skipped = result.total - result.copied - result.failed;
    result.duration = started.elapsed().as_secs_f64();
    Ok(result)
}

#[cfg(test)]
mod tests;
