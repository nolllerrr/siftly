use crate::{OperationGuard, OperationState};
use serde::Serialize;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{ipc::Channel, State, WebviewWindow};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
use tauri_plugin_updater::{Update, UpdaterExt};

#[derive(Default)]
pub(crate) struct UpdateState {
    busy: AtomicBool,
    pending: Mutex<Option<Update>>,
}

struct BusyGuard<'a>(&'a AtomicBool);
impl<'a> BusyGuard<'a> {
    fn acquire(busy: &'a AtomicBool) -> Result<Self, String> {
        busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "An update operation is already running")?;
        Ok(Self(busy))
    }
}
impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateInfo {
    version: Option<String>,
    notes: Option<String>,
    install_supported: bool,
}

#[derive(Clone, Serialize)]
pub(crate) struct UpdateProgress {
    phase: &'static str,
    downloaded: u64,
    total: Option<u64>,
}

fn validate_download_url(url: &tauri::Url) -> Result<(), String> {
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !url
            .path()
            .starts_with("/nolllerrr/siftly/releases/download/")
        || !url.path().ends_with(".exe")
    {
        return Err("Update installer must come from Siftly GitHub Releases".into());
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn check_for_updates(
    window: WebviewWindow,
    state: State<'_, UpdateState>,
) -> Result<UpdateInfo, String> {
    if window.label() != "main" {
        return Err("Unauthorized window".into());
    }
    let _busy = BusyGuard::acquire(&state.busy)?;
    *state
        .pending
        .lock()
        .map_err(|_| "Update state unavailable")? = None;
    // Network work is native; WebView cannot supply an endpoint or public key.
    let update = window
        .updater_builder()
        .timeout(Duration::from_secs(15))
        .configure_client(|client| client.https_only(true))
        .build()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?;
    if let Some(update) = &update {
        validate_download_url(&update.download_url)?;
    }
    let info = UpdateInfo {
        version: update.as_ref().map(|u| u.version.clone()),
        notes: update
            .as_ref()
            .and_then(|u| u.body.as_ref().map(|s| s.chars().take(8000).collect())),
        install_supported: !cfg!(debug_assertions),
    };
    *state
        .pending
        .lock()
        .map_err(|_| "Update state unavailable")? = update;
    Ok(info)
}

#[tauri::command]
pub(crate) async fn install_update(
    window: WebviewWindow,
    state: State<'_, UpdateState>,
    operations: State<'_, OperationState>,
    on_progress: Channel<UpdateProgress>,
) -> Result<bool, String> {
    if window.label() != "main" {
        return Err("Unauthorized window".into());
    }
    if cfg!(debug_assertions) {
        return Err("Install updates from a release build, not development mode".into());
    }
    let _busy = BusyGuard::acquire(&state.busy)?;
    let mut update = state
        .pending
        .lock()
        .map_err(|_| "Update state unavailable")?
        .clone()
        .ok_or("Check for updates first")?;
    validate_download_url(&update.download_url)?;
    operations.register("install-update")?;
    let guard = OperationGuard {
        state: operations.inner().clone(),
        id: "install-update".into(),
    };
    let version = update.version.clone();
    let confirmed = tauri::async_runtime::spawn_blocking(move || {
        window.dialog().message(format!("Download and install Siftly {version}?\n\nSiftly will close and restart. Current search results will not be saved."))
            .title("Siftly — update").parent(&window).buttons(MessageDialogButtons::OkCancel).blocking_show()
    }).await.map_err(|e| e.to_string())?;
    if !confirmed {
        return Ok(false);
    }
    update.timeout = Some(Duration::from_secs(600));
    let mut downloaded = 0u64;
    let mut last_sent = Instant::now();
    let bytes = update
        .download(
            |chunk, total| {
                downloaded = downloaded.saturating_add(chunk as u64);
                if last_sent.elapsed() >= Duration::from_millis(150) {
                    let _ = on_progress.send(UpdateProgress {
                        phase: "downloading",
                        downloaded,
                        total,
                    });
                    last_sent = Instant::now();
                }
            },
            || {
                let _ = on_progress.send(UpdateProgress {
                    phase: "verifying",
                    downloaded: 0,
                    total: None,
                });
            },
        )
        .await
        .map_err(|e| e.to_string())?;
    // download verifies both the signature and the signed app version before installation.
    let _ = on_progress.send(UpdateProgress {
        phase: "installing",
        downloaded: bytes.len() as u64,
        total: Some(bytes.len() as u64),
    });
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard; // Keep file operations blocked until the installer takes over.
        update.install(bytes).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_only_our_https_release_installers() {
        assert!(validate_download_url(
            &tauri::Url::parse(
                "https://github.com/nolllerrr/siftly/releases/download/v0.2.0/Siftly.exe"
            )
            .unwrap()
        )
        .is_ok());
        for url in [
            "http://github.com/nolllerrr/siftly/releases/download/v1/app.exe",
            "https://github.com.evil.test/nolllerrr/siftly/releases/download/v1/app.exe",
            "https://github.com/other/siftly/releases/download/v1/app.exe",
            "https://github.com/nolllerrr/siftly/releases/download/v1/app.zip",
            "https://user@github.com/nolllerrr/siftly/releases/download/v1/app.exe",
            "https://github.com/nolllerrr/siftly/releases/download/v1/app.exe?redirect=other",
            "https://github.com:444/nolllerrr/siftly/releases/download/v1/app.exe",
        ] {
            assert!(
                validate_download_url(&tauri::Url::parse(url).unwrap()).is_err(),
                "{url}"
            );
        }
    }
    #[test]
    fn busy_slot_is_released_after_error_or_cancel() {
        let busy = AtomicBool::new(false);
        {
            let _guard = BusyGuard::acquire(&busy).unwrap();
            assert!(BusyGuard::acquire(&busy).is_err());
        }
        assert!(BusyGuard::acquire(&busy).is_ok());
    }
}
