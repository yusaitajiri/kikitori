//! Kikitori: live transcription with local Whisper.

pub mod asr;
pub mod audio;
pub mod commands;
pub mod error;
pub mod events;
pub mod export;
pub mod hotkeys;
pub mod mic_test;
pub mod models;
pub mod net;
pub mod platform;
pub mod recorder;
pub mod screenshot;
pub mod segmenter;
pub mod session;
pub mod settings;
pub mod state;
pub mod tray;
pub mod updater;
pub mod vad;
pub mod window;

use std::sync::Arc;

use tauri::Manager;
use tauri_plugin_log::{RotationStrategy, Target, TargetKind, TimezoneStrategy};

use crate::asr::worker::AsrWorker;
use crate::state::{AppHost, AppState};

fn log_plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_log::Builder::new()
        .targets([
            Target::new(TargetKind::LogDir { file_name: Some("kikitori".into()) }),
            Target::new(TargetKind::Stdout),
        ])
        .rotation_strategy(RotationStrategy::KeepSome(5))
        .timezone_strategy(TimezoneStrategy::UseLocal)
        .max_file_size(5_000_000)
        .level(log::LevelFilter::Info)
        // whisper.cpp is chatty at info level.
        .level_for("whisper_rs", log::LevelFilter::Warn)
        .level_for("wasapi", log::LevelFilter::Warn)
        .level_for("tao", log::LevelFilter::Warn)
        .level_for("reqwest", log::LevelFilter::Warn)
        .build()
}

pub fn run() {
    // Before anything touches Vulkan (section 8).
    asr::engine::disable_vulkan_implicit_layers();
    net::ensure_crypto_provider();

    tauri::Builder::default()
        // Must be first so a second launch focuses the running window (FR-91).
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| window::show(app)))
        .plugin(log_plugin())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_process::init())
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(tauri_plugin_window_state::StateFlags::POSITION)
                .build(),
        )
        .setup(|app| {
            let handle = app.handle().clone();
            whisper_rs::install_logging_hooks();
            log_panics(&handle);
            tracing::info!(
                "Kikitori {} starting (Windows build {}, GPU backend compiled: {})",
                app.package_info().version,
                platform::os_build(),
                asr::engine::gpu_compiled()
            );

            updater::init(&handle)?;

            let worker = AsrWorker::spawn(Arc::new(AppHost { app: handle.clone() }));
            let st = AppState::new(&handle, worker);
            app.manage(st);
            let st = app.state::<AppState>();
            state::check_gpu_crash_marker(&handle, &st);

            let settings = st.settings.read().clone();
            commands::allow_asset_dir(&handle, std::path::Path::new(&settings.output.root));
            tray::create(&handle)?;
            hotkeys::register_all(&handle);
            window::apply_settings(&handle, &settings);
            window::apply_layout(&handle, settings.window.layout);
            window::show(&handle);
            recorder::spawn_ticker(handle.clone());

            if cpu_supported() {
                st.load_selected_model();
            } else {
                tracing::error!("CPU lacks AVX2; transcription is unavailable");
                events::notice(
                    &handle,
                    events::Notice::new(events::NoticeLevel::Error, "E_INTERNAL", "cpuUnsupported"),
                );
            }
            recorder::emit_state(&handle, &st);
            updater::check_on_launch(&handle);
            Ok(())
        })
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                window::on_close_requested(window.app_handle(), window, api);
            }
            tauri::WindowEvent::Resized(_) if window.label() == window::MAIN => {
                window::sync_webview_visibility(window.app_handle());
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_info,
            commands::get_settings,
            commands::set_settings,
            commands::list_audio_apps,
            commands::list_mic_devices,
            commands::watch_app_levels,
            commands::start_recording,
            commands::continue_recording,
            commands::stop_recording,
            commands::cancel_finishing,
            commands::pause_recording,
            commands::resume_recording,
            commands::switch_to_system,
            commands::switch_source,
            commands::take_screenshot,
            commands::add_cut,
            commands::list_windows,
            commands::set_shot_window,
            commands::mark_current_line,
            commands::mark_segment,
            commands::get_session,
            commands::copy_transcript,
            commands::export_markdown,
            commands::export_pdf,
            commands::export_typst,
            commands::open_path,
            commands::open_mic_privacy,
            commands::open_logs,
            commands::list_recoverable,
            commands::recover_session,
            commands::list_sessions,
            commands::delete_session,
            commands::delete_sessions,
            commands::list_projects,
            commands::create_project,
            commands::update_project,
            commands::delete_project,
            commands::set_project,
            commands::rename_session,
            commands::update_segment,
            commands::delete_segment,
            commands::delete_screenshot,
            commands::set_caption,
            commands::models_list,
            commands::model_download,
            commands::model_cancel,
            commands::model_select,
            commands::model_delete,
            commands::model_import,
            commands::run_benchmark,
            commands::retry_gpu,
            commands::mic_test,
            commands::complete_setup,
            commands::set_window_layout,
            commands::window_action,
            commands::quit_app,
            commands::install_update,
            commands::pick_folder,
            commands::pick_model_file,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Kikitori")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                let st = app.state::<AppState>();
                st.worker.shutdown();
            }
        });
}

/// The release build targets AVX2 (section 17 notes); older CPUs would crash in whisper.cpp.
fn cpu_supported() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        std::arch::is_x86_feature_detected!("avx2") && std::arch::is_x86_feature_detected!("fma")
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        true
    }
}

/// A panic anywhere is logged and the active session's log is flushed (section 16).
fn log_panics(app: &tauri::AppHandle) {
    let app = app.clone();
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!("panic: {info}");
        if let Some(st) = app.try_state::<AppState>()
            && let Some(guard) = st.recorder.try_lock()
            && let recorder::Phase::Recording(active) = &*guard
        {
            let _ = active.store.log.sync();
        }
        default(info);
    }));
}
