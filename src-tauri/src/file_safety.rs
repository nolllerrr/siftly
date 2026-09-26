use std::{fs::{self, File}, io, path::Path};

#[cfg(windows)]
pub fn handle_path(file: &File) -> io::Result<std::path::PathBuf> {
    use std::{os::windows::{ffi::OsStringExt, io::AsRawHandle}, ffi::OsString};
    use windows_sys::Win32::Storage::FileSystem::GetFinalPathNameByHandleW;
    let mut buffer = vec![0u16; 32768];
    // SAFETY: the handle is live and the writable buffer has the supplied size.
    let length = unsafe { GetFinalPathNameByHandleW(file.as_raw_handle(), buffer.as_mut_ptr(), buffer.len() as u32, 0) };
    if length == 0 { return Err(io::Error::last_os_error()); }
    if length as usize >= buffer.len() { return Err(io::Error::other("Resolved path is too long")); }
    Ok(OsString::from_wide(&buffer[..length as usize]).into())
}

// Pin every ancestor so a directory or junction cannot be swapped while using
// a validated path. A path comparison without live handles would have a race.
pub fn lock_directory(path: &Path) -> io::Result<Vec<File>> {
    #[cfg(windows)]
    {
        use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};
        use windows_sys::Win32::Storage::FileSystem::*;
        let canonical = fs::canonicalize(path)?;
        if path.as_os_str().to_string_lossy().starts_with(r"\\?\") && canonical != path {
            return Err(io::Error::other("Approved directory was redirected"));
        }
        let mut locks = Vec::new();
        let ancestors: Vec<_> = canonical.ancestors().collect();
        for directory in ancestors.into_iter().rev() {
            let file = OpenOptions::new().access_mode(FILE_READ_ATTRIBUTES)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS).open(directory)?;
            if !file.metadata()?.is_dir() || handle_path(&file)? != directory {
                return Err(io::Error::other("Directory changed or contains a redirected path"));
            }
            locks.push(file);
        }
        Ok(locks)
    }
    #[cfg(not(windows))]
    { let _ = path; Ok(Vec::new()) }
}

pub fn verify_open_file(file: &File, expected: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        let expected = if expected.as_os_str().to_string_lossy().starts_with(r"\\?\") { expected.to_path_buf() } else { fs::canonicalize(expected)? };
        if handle_path(file)? != expected { return Err(io::Error::other("Source path changed while opening")); }
    }
    if !file.metadata()?.is_file() { return Err(io::Error::other("Source is not a regular file")); }
    Ok(())
}

#[cfg(windows)]
pub fn copy_zone_identifier(source: &File, destination: &File) -> io::Result<()> {
    use std::{fs::OpenOptions, io::{Read, Write}, os::windows::fs::OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::*;
    fn stream(file: &File) -> io::Result<std::ffi::OsString> {
        let mut path = handle_path(file)?.into_os_string();
        path.push(":Zone.Identifier");
        Ok(path)
    }
    let input = match OpenOptions::new().read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE).open(stream(source)?) {
        Ok(input) => input,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    input.take(65537).read_to_end(&mut bytes)?;
    if bytes.len() > 65536 { return Err(io::Error::other("Zone.Identifier exceeds safety limit")); }
    let mut output = OpenOptions::new().write(true).create_new(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE).open(stream(destination)?)?;
    output.write_all(&bytes)?;
    output.sync_all()
}

// Never close and then delete by name on Windows: another process could replace
// that name. Mark this exact open object for deletion before releasing its handle.
#[cfg(windows)]
pub fn discard_output(file: &File, _path: &Path) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::*;
    let info = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: valid live handle, correct information class, structure and size.
    let success = unsafe { SetFileInformationByHandle(file.as_raw_handle(), FileDispositionInfo,
        &info as *const _ as *const _, std::mem::size_of_val(&info) as u32) };
    if success == 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

#[cfg(not(windows))]
pub fn discard_output(_file: &File, _path: &Path) -> io::Result<()> {
    Err(io::Error::other("Safe handle-based cleanup is only supported on Windows; partial file retained"))
}
