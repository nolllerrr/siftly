use crate::{copy_manager::CopyRequest, scanner::ScanRequest};
use serde_json::Value;
use std::{collections::HashSet, fs, path::PathBuf};

pub const MAX_FILES: usize = 10_000;
pub const MAX_ERRORS: usize = 1_000;
pub const MAX_DIRECTORIES: usize = 10_000;
pub const MAX_CHECKED: u64 = 1_000_000;

#[derive(Default)]
pub struct Session {
    pub source: Option<PathBuf>,
    pub destination: Option<PathBuf>,
    pub scanned: HashSet<PathBuf>,
}

impl Session {
    pub fn select(&mut self, kind: &str, path: PathBuf) -> Result<String, String> {
        if kind != "source" && kind != "destination" { return Err("Invalid folder kind".into()); }
        let path = fs::canonicalize(path).map_err(|error| error.to_string())?;
        if !path.is_dir() { return Err("Choose a directory".into()); }
        let display = path.to_string_lossy().into_owned();
        if kind == "source" {
            self.source = Some(path);
            self.scanned.clear();
        } else { self.destination = Some(path); }
        Ok(display)
    }

    pub fn authorize(&mut self, operation: &str, payload: &Value) -> Result<(), String> {
        match operation {
            "scan" => {
                self.scanned.clear();
                let request: ScanRequest = serde_json::from_value(payload.clone()).map_err(|e| e.to_string())?;
                let source = self.source.as_ref().ok_or("Choose source folder first")?;
                if PathBuf::from(&request.source) != *source || fs::canonicalize(source).map_err(|e| e.to_string())? != *source {
                    return Err("Source does not match the approved folder".into());
                }
                if request.extensions.is_empty() || request.extensions.len() > 100 || request.extensions.iter().any(|value| value.len() > 32) {
                    return Err("Use between 1 and 100 extensions, at most 32 bytes each".into());
                }
            }
            "copy" => {
                let request: CopyRequest = serde_json::from_value(payload.clone()).map_err(|e| e.to_string())?;
                let destination = self.destination.as_ref().ok_or("Choose destination folder first")?;
                let source = self.source.as_ref().ok_or("Choose source folder first")?;
                if PathBuf::from(&request.destination) != *destination || fs::canonicalize(destination).map_err(|e| e.to_string())? != *destination {
                    return Err("Destination does not match the approved folder".into());
                }
                if request.files.is_empty() || request.files.len() > MAX_FILES { return Err("Invalid file count".into()); }
                let mut unique = HashSet::new();
                for item in request.files {
                    let path = PathBuf::from(&item.path);
                    let canonical = fs::canonicalize(&path).map_err(|e| e.to_string())?;
                    if !canonical.starts_with(source) || !self.scanned.contains(&path) || canonical != path || !unique.insert(path) {
                        return Err("Only unique files from the last scan may be copied; scan again if files changed".into());
                    }
                }
            }
            _ => return Err("Unknown operation".into()),
        }
        Ok(())
    }

    pub fn remember_scan(&mut self, result: &Value) {
        self.scanned.clear();
        if let Some(files) = result["files"].as_array() {
            for file in files.iter().take(MAX_FILES) {
                if let Some(path) = file["path"].as_str() { self.scanned.insert(PathBuf::from(path)); }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_requests_without_native_folder_approval() {
        let mut session = Session::default();
        assert!(session.authorize("scan", &serde_json::json!({"source":"C:\\", "extensions":[".txt"]})).is_err());
        assert!(session.authorize("copy", &serde_json::json!({"destination":"C:\\", "files":[]})).is_err());
        assert!(session.authorize("other", &Value::Null).is_err());
    }
}
