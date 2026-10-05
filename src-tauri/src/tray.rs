//! Tray icon (FR-90). The app icon's bird, alone when idle, with its red ripples while recording
//! and grey ones while paused or finishing.

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, include_image};

use crate::events::UiState;
use crate::settings::Locale;
use crate::state::AppState;

pub const TRAY_ID: &str = "kikitori-tray";

struct Labels {
    start: &'static str,
    stop: &'static str,
    shot: &'static str,
    show: &'static str,
    quit: &'static str,
    idle_tip: &'static str,
    rec_tip: &'static str,
    finishing_tip: &'static str,
}

fn labels(locale: Locale) -> Labels {
    match locale {
        Locale::Ja => Labels {
            start: "開始",
            stop: "停止",
            shot: "スクショ",
            show: "ウィンドウを表示",
            quit: "終了",
            idle_tip: "Kikitori",
            rec_tip: "Kikitori — 録音中",
            finishing_tip: "Kikitori — 仕上げ中…",
        },
        Locale::En => Labels {
            start: "Start",
            stop: "Stop",
            shot: "Screenshot",
            show: "Show window",
            quit: "Quit",
            idle_tip: "Kikitori",
            rec_tip: "Kikitori — recording",
            finishing_tip: "Kikitori — finishing…",
        },
    }
}

// 32 px renders of icons/tray/*.svg; the bird stays put, so starting a recording only adds ripples.
const TRAY_IDLE: Image<'_> = include_image!("./icons/tray/idle.png");
const TRAY_RECORDING: Image<'_> = include_image!("./icons/tray/recording.png");
const TRAY_PAUSED: Image<'_> = include_image!("./icons/tray/paused.png");

/// The tray picture for a state: the ripples are the sound the bird is hearing.
fn icon_for(state: UiState) -> Image<'static> {
    match state {
        UiState::Recording => TRAY_RECORDING,
        UiState::Paused | UiState::Finishing => TRAY_PAUSED,
        _ => TRAY_IDLE,
    }
}

fn build_menu(app: &AppHandle, recording: bool) -> tauri::Result<Menu<tauri::Wry>> {
    let locale = app.try_state::<AppState>().map(|s| s.settings.read().locale).unwrap_or_default();
    let l = labels(locale);
    let toggle = MenuItem::with_id(app, "toggle", if recording { l.stop } else { l.start }, true, None::<&str>)?;
    let shot = MenuItem::with_id(app, "shot", l.shot, recording, None::<&str>)?;
    let show = MenuItem::with_id(app, "show", l.show, true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", l.quit, true, None::<&str>)?;
    Menu::with_items(app, &[&toggle, &shot, &show, &sep, &quit])
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app, false)?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Kikitori")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "toggle" => {
                let st = app.state::<AppState>();
                #[cfg(windows)]
                crate::recorder::toggle(app, &st);
            }
            "shot" => {
                let app = app.clone();
                std::thread::spawn(move || {
                    let st = app.state::<AppState>();
                    let _ = crate::recorder::take_screenshot(&app, &st);
                });
            }
            "show" => crate::window::show(app),
            "quit" => crate::window::request_quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                crate::window::toggle_visible(tray.app_handle());
            }
        });
    builder = builder.icon(icon_for(UiState::Ready));
    builder.build(app)?;
    Ok(())
}

/// Reflects the app state in the icon, tooltip and menu.
pub fn update(app: &AppHandle, state: UiState) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    let locale = app.try_state::<AppState>().map(|s| s.settings.read().locale).unwrap_or_default();
    let l = labels(locale);
    let tip = match state {
        UiState::Recording | UiState::Paused => l.rec_tip,
        UiState::Finishing => l.finishing_tip,
        _ => l.idle_tip,
    };
    let _ = tray.set_icon(Some(icon_for(state)));
    let _ = tray.set_tooltip(Some(tip));
    let recording = matches!(state, UiState::Recording | UiState::Paused);
    if let Ok(menu) = build_menu(app, recording) {
        let _ = tray.set_menu(Some(menu));
    }
}
