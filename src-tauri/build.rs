fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(
            tauri_build::AppManifest::new().commands(&[
                "choose_folder", "run_operation", "cancel_operation",
            ]),
        ),
    ).expect("Tauri build failed")
}
