fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "choose_folder",
            "run_operation",
            "cancel_operation",
            "check_for_updates",
            "install_update",
        ]),
    ))
    .expect("Tauri build failed")
}
