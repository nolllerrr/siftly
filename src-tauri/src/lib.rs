mod copy_manager;
mod file_safety;
mod scanner;
mod security;
mod updates;

use copy_manager::{copy_files, CopyRequest};
use scanner::{scan_files, ScanRequest};
use serde_json::Value;
use std::{
    collections::{hash_map::Entry, HashMap},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tauri::{ipc::Channel, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

#[derive(Clone, Default)]
struct OperationState {
    operations: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    session: Arc<Mutex<security::Session>>,
}

struct OperationGuard {
    state: OperationState,
    id: String,
}
impl Drop for OperationGuard {
    fn drop(&mut self) {
        self.state.remove(&self.id);
    }
}

impl OperationState {
    fn register(&self, op_id: &str) -> Result<Arc<AtomicBool>, String> {
        validate_operation_id(op_id)?;
        let flag = Arc::new(AtomicBool::new(false));
        let mut operations = self
            .operations
            .lock()
            .map_err(|_| "Operation state is unavailable")?;
        if !operations.is_empty() {
            return Err("Another operation or dialog is already active".into());
        }
        match operations.entry(op_id.to_string()) {
            Entry::Occupied(_) => Err("Operation id is already running".into()),
            Entry::Vacant(entry) => {
                entry.insert(flag.clone());
                Ok(flag)
            }
        }
    }

    fn remove(&self, op_id: &str) {
        if let Ok(mut operations) = self.operations.lock() {
            operations.remove(op_id);
        }
    }

    fn cancel(&self, op_id: &str) -> Result<(), String> {
        validate_operation_id(op_id)?;
        let operations = self
            .operations
            .lock()
            .map_err(|_| "Operation state is unavailable")?;
        if let Some(flag) = operations.get(op_id) {
            flag.store(true, Ordering::Relaxed);
        }
        Ok(())
    }
}

fn validate_operation_id(op_id: &str) -> Result<(), String> {
    if op_id.is_empty()
        || op_id.len() > 128
        || !op_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        return Err("Invalid operation id".into());
    }
    Ok(())
}

fn execute_operation(
    operation: &str,
    payload: Value,
    cancelled: &AtomicBool,
    mut on_progress: impl FnMut(Value),
) -> Result<Value, String> {
    match operation {
        "scan" => {
            let request: ScanRequest = serde_json::from_value(payload)
                .map_err(|error| format!("Invalid scan request: {error}"))?;
            let result = scan_files(
                request,
                || cancelled.load(Ordering::Relaxed),
                |progress| {
                    let mut event =
                        serde_json::to_value(progress).expect("scan progress is serializable");
                    event["type"] = "scan_progress".into();
                    on_progress(event);
                },
            )?;
            serde_json::to_value(result).map_err(|error| error.to_string())
        }
        "copy" => {
            let request: CopyRequest = serde_json::from_value(payload)
                .map_err(|error| format!("Invalid copy request: {error}"))?;
            let result = copy_files(
                request,
                || cancelled.load(Ordering::Relaxed),
                |progress| {
                    let mut event =
                        serde_json::to_value(progress).expect("copy progress is serializable");
                    event["type"] = "copy_progress".into();
                    on_progress(event);
                },
            )?;
            serde_json::to_value(result).map_err(|error| error.to_string())
        }
        _ => Err(format!("Unknown operation: {operation}")),
    }
}

#[tauri::command]
async fn run_operation(
    state: State<'_, OperationState>,
    window: WebviewWindow,
    operation: String,
    payload: Value,
    op_id: String,
    on_progress: Channel<Value>,
) -> Result<Value, String> {
    if window.label() != "main" {
        return Err("Unauthorized window".into());
    }
    if payload.to_string().len() > 8 * 1024 * 1024 {
        return Err("Request is too large".into());
    }
    let state = state.inner().clone();
    let cancelled = state.register(&op_id)?;
    let guard = OperationGuard {
        state: state.clone(),
        id: op_id,
    };
    state
        .session
        .lock()
        .map_err(|_| "Session unavailable")?
        .authorize(&operation, &payload)?;
    let task = tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        let (source, destination) = {
            let session = state.session.lock().map_err(|_| "Session unavailable")?;
            (session.source.clone().ok_or("Choose source folder first")?, session.destination.clone())
        };
        let _source_locks = file_safety::lock_directory(&source).map_err(|e| e.to_string())?;
        let _destination_locks = if operation == "copy" {
            Some(file_safety::lock_directory(destination.as_ref().ok_or("Choose destination folder first")?).map_err(|e| e.to_string())?)
        } else { None };
        // Revalidate after acquiring directory locks, before I/O.
        state.session.lock().map_err(|_| "Session unavailable")?.authorize(&operation, &payload)?;
        let result = execute_operation(&operation, payload, &cancelled, |event| {
            // A closed window should stop its background operation too.
            if on_progress.send(event).is_err() {
                cancelled.store(true, Ordering::Relaxed);
            }
        })?;
        if operation == "scan" { state.session.lock().map_err(|_| "Session unavailable")?.remember_scan(&result); }
        Ok::<Value, String>(result)
    })
    .await;
    task.map_err(|error| format!("Operation task failed: {error}"))?
}

#[tauri::command]
async fn choose_folder(
    state: State<'_, OperationState>,
    window: WebviewWindow,
    kind: String,
) -> Result<Option<String>, String> {
    if window.label() != "main" || (kind != "source" && kind != "destination") {
        return Err("Invalid folder request".into());
    }
    let state = state.inner().clone();
    state.register("folder-dialog")?;
    let guard = OperationGuard {
        state: state.clone(),
        id: "folder-dialog".into(),
    };
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        let folder = window
            .dialog()
            .file()
            .set_parent(&window)
            .set_title(if kind == "source" {
                "Choose source folder"
            } else {
                "Choose destination folder"
            })
            .blocking_pick_folder();
        match folder {
            Some(folder) => {
                let path = folder.into_path().map_err(|e| e.to_string())?;
                state
                    .session
                    .lock()
                    .map_err(|_| "Session unavailable")?
                    .select(&kind, path)
                    .map(Some)
            }
            None => Ok(None),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn cancel_operation(state: State<'_, OperationState>, op_id: String) -> Result<(), String> {
    state.cancel(&op_id)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(OperationState::default())
        .manage(updates::UpdateState::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            choose_folder,
            run_operation,
            cancel_operation,
            updates::check_for_updates,
            updates::install_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running Siftly");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_id_cannot_replace_a_cancellation_flag() {
        let state = OperationState::default();
        let flag = state.register("copy-1").unwrap();
        assert!(state.register("copy-1").is_err());
        assert!(state.register("scan-2").is_err());
        state.cancel("copy-1").unwrap();
        assert!(flag.load(Ordering::Relaxed));
        state.remove("copy-1");
        assert!(!state.register("copy-1").unwrap().load(Ordering::Relaxed));
        assert!(state.cancel("../../invalid").is_err());
        state.cancel("already-finished").unwrap();
    }

    #[test]
    fn operation_guard_releases_slot_on_error() {
        let state = OperationState::default();
        state.register("first").unwrap();
        {
            let _guard = OperationGuard {
                state: state.clone(),
                id: "first".into(),
            };
            assert!(state.register("second").is_err());
        }
        assert!(state.register("second").is_ok());
    }

    #[test]
    fn command_dispatch_is_native_and_rejects_invalid_requests() {
        let flag = AtomicBool::new(false);
        let result = execute_operation(
            "copy",
            serde_json::json!({"files": [], "destination": ""}),
            &flag,
            |_| {},
        )
        .unwrap();
        assert_eq!(result["type"], "copy_result");
        assert_eq!(result["copied"], 0);
        assert!(execute_operation("copy", serde_json::json!({}), &flag, |_| {}).is_err());
        assert!(execute_operation("other", serde_json::json!({}), &flag, |_| {}).is_err());
    }
}
