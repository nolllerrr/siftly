//! Recover from stale WebView2 registration without changing Windows or its registry.
//! Windows Sandbox can expose a registry version whose runtime folder is missing,
//! even though a newer, usable runtime is present in the Microsoft installation.

use std::{
    fs,
    os::windows::{ffi::OsStrExt, fs::MetadataExt},
    path::{Path, PathBuf},
};
use webview2_com::{
    take_pwstr, Microsoft::Web::WebView2::Win32::GetAvailableCoreWebView2BrowserVersionString,
};
use windows::{
    core::{PCWSTR, PWSTR},
    Win32::UI::Shell::{
        FOLDERID_LocalAppData, FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX86,
        SHGetKnownFolderPath, KF_FLAG_DEFAULT,
    },
};

const RUNTIME_OVERRIDE: &str = "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER";

pub fn prepare() {
    // An explicit user/deployment override takes precedence, including an empty one.
    if std::env::var_os(RUNTIME_OVERRIDE).is_some() || tauri::webview_version().is_ok() {
        return;
    }

    if let Some(folder) = find_runtime(&installation_roots(), runtime_available) {
        // Process-local only. run() calls us before starting the Tauri runtime.
        std::env::set_var(RUNTIME_OVERRIDE, folder);
    }
}

fn installation_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for folder_id in [
        FOLDERID_ProgramFilesX86,
        FOLDERID_ProgramFiles,
        FOLDERID_LocalAppData,
    ] {
        // SAFETY: a constant known-folder GUID and no borrowed handle are passed;
        // take_pwstr releases the returned COM allocation.
        if let Ok(folder) = unsafe { SHGetKnownFolderPath(&folder_id, KF_FLAG_DEFAULT, None) } {
            let root = PathBuf::from(take_pwstr(folder)).join(r"Microsoft\EdgeWebView\Application");
            if !roots.contains(&root) {
                roots.push(root);
            }
        }
    }
    roots
}

fn runtime_available(folder: &Path) -> bool {
    let encoded: Vec<u16> = folder.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut version = PWSTR::null();
    // SAFETY: the path is null-terminated and lives for the entire call; version is
    // a writable out-pointer. take_pwstr releases any returned COM allocation.
    let result = unsafe {
        GetAvailableCoreWebView2BrowserVersionString(PCWSTR(encoded.as_ptr()), &mut version)
    };
    let version = take_pwstr(version);
    result.is_ok() && parse_version(&version).is_some()
}

fn find_runtime(roots: &[PathBuf], mut probe: impl FnMut(&Path) -> bool) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    for root in roots {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let Some(version) = entry.file_name().to_str().and_then(parse_version) else {
                continue;
            };
            let folder = entry.path();
            if regular_path(&folder, true)
                && regular_path(&folder.join("msedgewebview2.exe"), false)
            {
                candidates.push((version, folder));
            }
        }
    }
    // Numeric ordering matters: .100 is newer than .99. Probe each candidate so
    // an incomplete or incompatible newest installation cannot hide a usable one.
    candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.0));
    candidates
        .into_iter()
        .map(|(_, folder)| folder)
        .find(|folder| probe(folder))
}

fn regular_path(path: &Path, directory: bool) -> bool {
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
    fs::symlink_metadata(path).is_ok_and(|metadata| {
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0
            && if directory {
                metadata.is_dir()
            } else {
                metadata.is_file()
            }
    })
}

fn parse_version(value: &str) -> Option<[u32; 4]> {
    let parts: Vec<_> = value.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    let mut version = [0; 4];
    for (index, part) in parts.into_iter().enumerate() {
        if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        version[index] = part.parse().ok()?;
    }
    (version != [0; 4]).then_some(version)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let folder = std::env::temp_dir().join(format!(
                "siftly-webview-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&folder).unwrap();
            Self(folder)
        }

        fn add(&self, version: &str, executable: bool) -> PathBuf {
            let folder = self.0.join(version);
            fs::create_dir_all(&folder).unwrap();
            if executable {
                fs::write(folder.join("msedgewebview2.exe"), []).unwrap();
            }
            folder
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn stale_registered_version_is_not_needed_to_find_newer_runtime() {
        let fixture = Fixture::new();
        let expected = fixture.add("154.0.4258.53", true);
        assert_eq!(
            find_runtime(std::slice::from_ref(&fixture.0), |_| true),
            Some(expected)
        );
    }

    #[test]
    fn selects_newest_numeric_version_across_installations() {
        let machine = Fixture::new();
        let user = Fixture::new();
        machine.add("154.0.4258.99", true);
        let expected = user.add("154.0.4258.100", true);
        assert_eq!(
            find_runtime(&[machine.0.clone(), user.0.clone()], |_| true),
            Some(expected)
        );
    }

    #[test]
    fn falls_back_when_latest_runtime_cannot_be_loaded() {
        let fixture = Fixture::new();
        let expected = fixture.add("154.0.4258.48", true);
        let unusable = fixture.add("154.0.4258.53", true);
        assert_eq!(
            find_runtime(std::slice::from_ref(&fixture.0), |folder| folder
                != unusable),
            Some(expected)
        );
    }

    #[test]
    fn ignores_incomplete_installations_and_unrelated_directories() {
        let fixture = Fixture::new();
        fixture.add("154.0.4258.53", false);
        fixture.add("SetupMetrics", true);
        fixture.add("154.0.4258.53-backup", true);
        assert_eq!(
            find_runtime(std::slice::from_ref(&fixture.0), |_| panic!(
                "no valid candidate"
            )),
            None
        );
    }

    #[test]
    fn absence_of_runtime_leaves_normal_missing_runtime_handling_in_place() {
        let fixture = Fixture::new();
        assert_eq!(find_runtime(&[fixture.0.join("missing")], |_| true), None);
    }

    #[test]
    fn rejects_non_versions_and_overflow() {
        for value in [
            "",
            "0.0.0.0",
            "154.0.53",
            "154.0.4258.53.1",
            "+154.0.4258.53",
            "154.0.4258.4294967296",
        ] {
            assert_eq!(parse_version(value), None, "{value}");
        }
        assert_eq!(parse_version("154.0.4258.53"), Some([154, 0, 4258, 53]));
    }

    #[test]
    #[ignore = "requires an installed WebView2 runtime; read-only native loader smoke test"]
    fn native_loader_accepts_a_runtime_from_the_known_installation_folders() {
        assert!(find_runtime(&installation_roots(), runtime_available).is_some());
    }
}
