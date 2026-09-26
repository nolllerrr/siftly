use chrono::{DateTime, Local, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::{Instant, SystemTime},
};

#[derive(Debug, Deserialize)]
pub struct ScanRequest {
    pub source: String,
    #[serde(default)]
    pub extensions: Vec<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    #[serde(default = "default_recursive")]
    pub recursive: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct FileResult {
    pub path: String,
    pub name: String,
    pub extension: String,
    pub size: u64,
    pub modified_time: String,
    pub parent_folder: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct OperationError {
    pub path: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ScanProgress {
    pub files_checked: u64,
    pub files_matched: usize,
    pub current_directory: String,
}

#[derive(Debug, Serialize)]
pub struct ScanResult {
    #[serde(rename = "type")]
    pub event_type: &'static str,
    pub files: Vec<FileResult>,
    pub errors: Vec<OperationError>,
    pub files_checked: u64,
    pub cancelled: bool,
    pub truncated: bool,
    pub duration: f64,
}

fn default_recursive() -> bool {
    true
}

pub fn normalize_extensions(extensions: &[String]) -> HashSet<String> {
    extensions
        .iter()
        .filter_map(|extension| {
            let value = extension.trim().to_lowercase();
            if value.is_empty() {
                None
            } else if value.starts_with('.') {
                Some(value)
            } else {
                Some(format!(".{value}"))
            }
        })
        .collect()
}

fn parse_boundary(value: Option<&str>, field: &str) -> Result<Option<DateTime<Utc>>, String> {
    value
        .filter(|value| !value.is_empty())
        .map(|value| {
            DateTime::parse_from_rfc3339(value)
                .map(|date| date.with_timezone(&Utc))
                .map_err(|error| format!("Invalid {field}: {error}"))
        })
        .transpose()
}

fn format_modified_time(value: SystemTime) -> String {
    let local: DateTime<Local> = value.into();
    if local.timestamp_subsec_micros() == 0 {
        local.to_rfc3339_opts(SecondsFormat::Secs, false)
    } else {
        local.to_rfc3339_opts(SecondsFormat::Micros, false)
    }
}

fn is_within_range(
    modified: DateTime<Utc>,
    date_from: Option<DateTime<Utc>>,
    date_to: Option<DateTime<Utc>>,
) -> bool {
    !date_from.is_some_and(|date| modified < date) && !date_to.is_some_and(|date| modified > date)
}

fn absolute_source(source: &str) -> Result<PathBuf, String> {
    fs::canonicalize(source).map_err(|error| format!("Cannot resolve source folder: {error}"))
}

struct ScanLimits {
    files: usize,
    errors: usize,
    directories: usize,
    checked: u64,
}
impl Default for ScanLimits {
    fn default() -> Self {
        Self {
            files: crate::security::MAX_FILES,
            errors: crate::security::MAX_ERRORS,
            directories: crate::security::MAX_DIRECTORIES,
            checked: crate::security::MAX_CHECKED,
        }
    }
}

pub fn scan_files<C, P>(
    request: ScanRequest,
    is_cancelled: C,
    on_progress: P,
) -> Result<ScanResult, String>
where
    C: Fn() -> bool,
    P: FnMut(ScanProgress),
{
    scan_files_bounded(request, is_cancelled, on_progress, ScanLimits::default())
}

fn scan_files_bounded<C: Fn() -> bool, P: FnMut(ScanProgress)>(
    request: ScanRequest,
    is_cancelled: C,
    mut on_progress: P,
    limits: ScanLimits,
) -> Result<ScanResult, String> {
    let started = Instant::now();
    let source_path = absolute_source(&request.source)?;
    if !source_path.is_dir() {
        return Err(format!("Source folder does not exist: {}", request.source));
    }

    let allowed = normalize_extensions(&request.extensions);
    let date_from = parse_boundary(request.date_from.as_deref(), "date_from")?;
    let date_to = parse_boundary(request.date_to.as_deref(), "date_to")?;
    if date_from.zip(date_to).is_some_and(|(from, to)| from > to) {
        return Err("Invalid date range".into());
    }
    let mut files = Vec::new();
    let mut errors = Vec::new();
    let mut files_checked = 0_u64;
    let mut cancelled = false;
    let mut truncated = false;
    let mut directory_count = 1;
    let mut directories = vec![source_path.clone()];

    'scan: while let Some(directory) = directories.pop() {
        if errors.len() >= limits.errors {
            truncated = true;
            break;
        }
        if is_cancelled() {
            cancelled = true;
            break;
        }

        on_progress(ScanProgress {
            files_checked,
            files_matched: files.len(),
            current_directory: directory.to_string_lossy().into_owned(),
        });

        let _directory_locks = match crate::file_safety::lock_directory(&directory) {
            Ok(locks)
                if fs::canonicalize(&directory)
                    .is_ok_and(|p| p == directory && p.starts_with(&source_path)) =>
            {
                locks
            }
            _ => {
                errors.push(OperationError {
                    path: directory.to_string_lossy().into_owned(),
                    message: "Directory changed or cannot be safely opened".into(),
                });
                continue;
            }
        };
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                errors.push(OperationError {
                    path: directory.to_string_lossy().into_owned(),
                    message: error.to_string(),
                });
                continue;
            }
        };

        for entry in entries {
            if files.len() >= limits.files
                || errors.len() >= limits.errors
                || files_checked >= limits.checked
            {
                truncated = true;
                break 'scan;
            }
            if is_cancelled() {
                cancelled = true;
                break;
            }

            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    errors.push(OperationError {
                        path: directory.to_string_lossy().into_owned(),
                        message: error.to_string(),
                    });
                    continue;
                }
            };
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) => {
                    errors.push(OperationError {
                        path: path.to_string_lossy().into_owned(),
                        message: error.to_string(),
                    });
                    continue;
                }
            };

            if file_type.is_symlink() {
                continue;
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
                match entry.metadata() {
                    Ok(metadata)
                        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 =>
                    {
                        continue
                    }
                    Err(error) => {
                        errors.push(OperationError {
                            path: path.to_string_lossy().into_owned(),
                            message: error.to_string(),
                        });
                        continue;
                    }
                    _ => {}
                }
            }
            if file_type.is_dir() {
                if request.recursive {
                    if directory_count >= limits.directories {
                        truncated = true;
                        break 'scan;
                    }
                    directory_count += 1;
                    directories.push(path);
                }
                continue;
            }
            if !file_type.is_file() {
                continue;
            }

            files_checked += 1;
            let extension = path
                .extension()
                .map(|value| format!(".{}", value.to_string_lossy().to_lowercase()))
                .unwrap_or_default();
            if !allowed.is_empty() && !allowed.contains(&extension) {
                continue;
            }

            let metadata = match path.metadata() {
                Ok(metadata) => metadata,
                Err(error) => {
                    errors.push(OperationError {
                        path: path.to_string_lossy().into_owned(),
                        message: error.to_string(),
                    });
                    continue;
                }
            };
            let modified = match metadata.modified() {
                Ok(modified) => modified,
                Err(error) => {
                    errors.push(OperationError {
                        path: path.to_string_lossy().into_owned(),
                        message: error.to_string(),
                    });
                    continue;
                }
            };
            let modified_utc: DateTime<Utc> = modified.into();
            if !is_within_range(modified_utc, date_from, date_to) {
                continue;
            }

            files.push(FileResult {
                path: path.to_string_lossy().into_owned(),
                name: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                extension,
                size: metadata.len(),
                modified_time: format_modified_time(modified),
                parent_folder: path
                    .parent()
                    .unwrap_or_else(|| Path::new(""))
                    .to_string_lossy()
                    .into_owned(),
            });

            if files_checked.is_multiple_of(100) {
                on_progress(ScanProgress {
                    files_checked,
                    files_matched: files.len(),
                    current_directory: directory.to_string_lossy().into_owned(),
                });
            }
        }

        if cancelled {
            break;
        }
    }

    if truncated {
        errors.push(OperationError { path: source_path.to_string_lossy().into_owned(), message: "Safety limit reached: partial results only. Narrow the folder, extensions or date range.".into() });
    }

    on_progress(ScanProgress {
        files_checked,
        files_matched: files.len(),
        current_directory: source_path.to_string_lossy().into_owned(),
    });

    Ok(ScanResult {
        event_type: "scan_result",
        files,
        errors,
        files_checked,
        cancelled,
        truncated,
        duration: started.elapsed().as_secs_f64(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_scan_returns_explicit_partial_results() {
        let temporary = TempDirectory::new();
        for name in ["one.mp4", "two.mp4", "three.mp4"] {
            fs::write(temporary.0.join(name), b"x").unwrap();
        }
        let result = scan_files_bounded(
            request(&temporary.0, true),
            || false,
            |_| {},
            ScanLimits {
                files: 2,
                errors: 10,
                directories: 10,
                checked: 100,
            },
        )
        .unwrap();
        assert_eq!(result.files.len(), 2);
        assert!(result.truncated);
        assert!(!result.cancelled);
        assert!(result
            .errors
            .last()
            .unwrap()
            .message
            .contains("Safety limit"));
    }

    #[test]
    fn bounded_scan_limits_directory_queue_and_checked_files() {
        let temporary = TempDirectory::new();
        fs::create_dir(temporary.0.join("nested")).unwrap();
        let result = scan_files_bounded(
            request(&temporary.0, true),
            || false,
            |_| {},
            ScanLimits {
                files: 10,
                errors: 10,
                directories: 1,
                checked: 100,
            },
        )
        .unwrap();
        assert!(result.truncated);
        fs::write(temporary.0.join("one.mp4"), b"x").unwrap();
        fs::write(temporary.0.join("two.mp4"), b"x").unwrap();
        let result = scan_files_bounded(
            request(&temporary.0, false),
            || false,
            |_| {},
            ScanLimits {
                files: 10,
                errors: 10,
                directories: 10,
                checked: 1,
            },
        )
        .unwrap();
        assert!(result.truncated);
        assert_eq!(result.files_checked, 1);
    }
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    struct TempDirectory(PathBuf);

    impl TempDirectory {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "siftly-scanner-test-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("temporary directory should be created");
            Self(path)
        }
    }

    impl Drop for TempDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn request(source: &Path, recursive: bool) -> ScanRequest {
        ScanRequest {
            source: source.to_string_lossy().into_owned(),
            extensions: vec!["MP4".into(), ".mKv".into()],
            date_from: None,
            date_to: None,
            recursive,
        }
    }

    #[test]
    fn normalizes_extensions() {
        let values = vec![" MP4 ".into(), ".mKv".into(), "".into(), "MP4".into()];
        assert_eq!(
            normalize_extensions(&values),
            HashSet::from([".mp4".to_string(), ".mkv".to_string()])
        );
    }

    #[test]
    fn date_boundaries_are_inclusive() {
        let start = parse_boundary(Some("2026-09-10T00:00:00.000Z"), "date_from")
            .unwrap()
            .unwrap();
        let end = parse_boundary(Some("2026-09-20T23:59:59.999Z"), "date_to")
            .unwrap()
            .unwrap();

        assert!(is_within_range(start, Some(start), Some(end)));
        assert!(is_within_range(end, Some(start), Some(end)));
        assert!(!is_within_range(
            start - chrono::Duration::milliseconds(1),
            Some(start),
            Some(end)
        ));
        assert!(!is_within_range(
            end + chrono::Duration::milliseconds(1),
            Some(start),
            Some(end)
        ));
    }

    #[test]
    fn scans_recursively_and_non_recursively() {
        let temporary = TempDirectory::new();
        let nested = temporary.0.join("nested");
        fs::create_dir(&nested).unwrap();
        fs::write(temporary.0.join("top.MP4"), b"top").unwrap();
        fs::write(nested.join("nested.mkv"), b"nested").unwrap();
        fs::write(temporary.0.join("ignored.txt"), b"ignored").unwrap();

        let recursive = scan_files(request(&temporary.0, true), || false, |_| {}).unwrap();
        let shallow = scan_files(request(&temporary.0, false), || false, |_| {}).unwrap();
        let recursive_names: HashSet<_> = recursive
            .files
            .iter()
            .map(|file| file.name.as_str())
            .collect();

        assert_eq!(recursive_names, HashSet::from(["top.MP4", "nested.mkv"]));
        assert_eq!(recursive.files_checked, 3);
        assert_eq!(shallow.files.len(), 1);
        assert_eq!(shallow.files[0].name, "top.MP4");
        assert_eq!(shallow.files_checked, 2);
    }

    #[test]
    fn cancellation_returns_partial_results_and_final_progress() {
        let temporary = TempDirectory::new();
        fs::write(temporary.0.join("first.mp4"), b"first").unwrap();
        fs::write(temporary.0.join("second.mp4"), b"second").unwrap();
        let checks = AtomicUsize::new(0);
        let mut progress = Vec::new();

        let result = scan_files(
            request(&temporary.0, true),
            || checks.fetch_add(1, Ordering::Relaxed) > 1,
            |event| progress.push(event),
        )
        .unwrap();

        assert!(result.cancelled);
        assert_eq!(result.files_checked, 1);
        assert_eq!(result.files.len(), 1);
        assert_eq!(progress.last().unwrap().files_checked, result.files_checked);
        assert_eq!(progress.last().unwrap().files_matched, result.files.len());
    }

    #[test]
    fn scan_result_contract_matches_fixed_fixture() {
        use std::fs::{FileTimes, OpenOptions};
        use std::time::Duration;

        let temporary = TempDirectory::new();
        let mut expected = Vec::new();
        for (relative, bytes) in [
            ("top.MP4", b"top".as_slice()),
            ("one/clip.mkv", b"one"),
            ("two/clip.mkv", b"second"),
        ] {
            let path = relative
                .split('/')
                .fold(temporary.0.clone(), |path, part| path.join(part));
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, bytes).unwrap();
            OpenOptions::new()
                .write(true)
                .open(&path)
                .unwrap()
                .set_times(
                    FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(1_700_000_000)),
                )
                .unwrap();
            expected.push((fs::canonicalize(path).unwrap(), bytes.len() as u64));
        }
        fs::write(temporary.0.join("ignored.txt"), b"ignored").unwrap();
        let mut result = scan_files(request(&temporary.0, true), || false, |_| {}).unwrap();
        result
            .files
            .sort_by(|left, right| left.path.cmp(&right.path));
        expected.sort_by(|left, right| left.0.cmp(&right.0));
        assert_eq!(result.event_type, "scan_result");
        assert_eq!(result.files_checked, 4);
        assert!(!result.cancelled);
        assert!(result.errors.is_empty());
        assert_eq!(result.files.len(), expected.len());
        for (file, (path, size)) in result.files.iter().zip(expected) {
            assert_eq!(file.path, path.to_string_lossy());
            assert_eq!(file.name, path.file_name().unwrap().to_string_lossy());
            assert_eq!(file.parent_folder, path.parent().unwrap().to_string_lossy());
            assert_eq!(file.size, size);
            assert_eq!(
                file.extension,
                if file.name == "top.MP4" {
                    ".mp4"
                } else {
                    ".mkv"
                }
            );
            assert_eq!(
                DateTime::parse_from_rfc3339(&file.modified_time)
                    .unwrap()
                    .timestamp(),
                1_700_000_000
            );
            let serialized = serde_json::to_value(file).unwrap();
            assert_eq!(serialized.as_object().unwrap().len(), 6);
            assert_eq!(serialized["path"], file.path);
            assert_eq!(serialized["modified_time"], file.modified_time);
        }
    }
}
