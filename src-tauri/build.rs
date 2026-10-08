// Registering the commands here makes Tauri generate one `allow-<command>` permission per
// command; capabilities/default.json then grants the main window exactly these (section 14).
const COMMANDS: &[&str] = &[
    "get_app_info",
    "get_settings",
    "set_settings",
    "list_audio_apps",
    "list_mic_devices",
    "watch_app_levels",
    "start_recording",
    "stop_recording",
    "cancel_finishing",
    "pause_recording",
    "resume_recording",
    "switch_to_system",
    "switch_source",
    "take_screenshot",
    "add_cut",
    "list_windows",
    "set_shot_window",
    "mark_current_line",
    "mark_segment",
    "get_session",
    "copy_transcript",
    "export_markdown",
    "export_pdf",
    "export_typst",
    "open_path",
    "open_mic_privacy",
    "open_logs",
    "list_recoverable",
    "recover_session",
    "list_sessions",
    "delete_session",
    "delete_sessions",
    "list_projects",
    "create_project",
    "update_project",
    "delete_project",
    "set_project",
    "rename_session",
    "update_segment",
    "delete_segment",
    "delete_screenshot",
    "set_caption",
    "models_list",
    "model_download",
    "model_cancel",
    "model_select",
    "model_delete",
    "model_import",
    "run_benchmark",
    "retry_gpu",
    "mic_test",
    "complete_setup",
    "set_window_layout",
    "window_action",
    "quit_app",
    "install_update",
    "pick_folder",
    "pick_model_file",
];

fn main() {
    // tauri-build copies icons/icon.ico into the exe (the taskbar shows it) but only reruns when the
    // config changes, and include_image! reads the tray pictures only when the crate compiles. Rerun
    // (and so rebuild) when any of them change.
    println!("cargo:rerun-if-changed=icons/icon.ico");
    println!("cargo:rerun-if-changed=icons/tray");
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("tauri build script failed");
}
