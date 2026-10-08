//! Settings (section 15), stored with `tauri-plugin-store` and validated on load:
//! unknown keys are dropped, missing or invalid values get defaults, ranges are clamped.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::export::LabelMode;

pub const SETTINGS_FILE: &str = "settings.json";
pub const STATE_FILE: &str = "state.json";
pub const MAX_VOCABULARY: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Locale {
    #[default]
    Ja,
    En,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    #[default]
    Compact,
    Expanded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SourceMode {
    Mic,
    #[default]
    System,
    App,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    Ja,
    En,
    Auto,
}

impl Language {
    pub fn as_str(self) -> &'static str {
        match self {
            Language::Ja => "ja",
            Language::En => "en",
            Language::Auto => "auto",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ScreenshotTarget {
    CursorMonitor,
    AllMonitors,
    /// The recorded app's window; the cursor's screen when no app is being recorded (system
    /// audio, the mic alone, or the app not open yet).
    #[default]
    AppWindow,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WindowSettings {
    pub always_on_top: bool,
    pub layout: Layout,
}

impl Default for WindowSettings {
    fn default() -> Self {
        // Off until pinned in the title bar.
        Self { always_on_top: false, layout: Layout::Compact }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateSettings {
    pub check: bool,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self { check: true }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct LastApp {
    pub exe: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SourceSettings {
    pub mode: SourceMode,
    pub include_mic: bool,
    pub mic_device_id: Option<String>,
    /// The last app picked in app mode, so the hotkey can start with the last-used source.
    pub app: Option<LastApp>,
}

impl Default for SourceSettings {
    fn default() -> Self {
        Self { mode: SourceMode::System, include_mic: true, mic_device_id: None, app: None }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VadSettings {
    pub start_threshold: f32,
    pub hangover_ms: u32,
}

impl Default for VadSettings {
    fn default() -> Self {
        Self { start_threshold: 0.5, hangover_ms: 640 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScreenshotSettings {
    pub target: ScreenshotTarget,
    pub exclude_self: bool,
    pub sound: bool,
}

impl Default for ScreenshotSettings {
    fn default() -> Self {
        Self { target: ScreenshotTarget::AppWindow, exclude_self: true, sound: false }
    }
}

/// Global shortcuts; an empty one is off. All are off until the user sets them in Settings:
/// obvious defaults such as `Ctrl+Alt+R` are often held by another app already.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Hotkeys {
    pub toggle: String,
    pub screenshot: String,
    /// Mark the line being said as important (FR-07).
    pub mark: String,
    /// Start a new part (FR-06).
    pub cut: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OutputSettings {
    /// Empty means `Documents\Kikitori`; resolved at load.
    pub root: String,
    pub title_template: String,
}

impl Default for OutputSettings {
    fn default() -> Self {
        Self { root: String::new(), title_template: "{app}".into() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ExportSettings {
    pub timestamps: bool,
    pub labels: LabelMode,
    pub merge_paragraphs: bool,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self { timestamps: true, labels: LabelMode::Auto, merge_paragraphs: true }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CopySettings {
    pub screenshot_markers: bool,
}

impl Default for CopySettings {
    fn default() -> Self {
        Self { screenshot_markers: true }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub locale: Locale,
    pub window: WindowSettings,
    pub updates: UpdateSettings,
    pub source: SourceSettings,
    pub echo_guard: bool,
    pub vad: VadSettings,
    pub language: Language,
    pub model_id: String,
    pub use_gpu: bool,
    pub accuracy_first: bool,
    pub partials: bool,
    pub vocabulary: Vec<String>,
    pub hallucination_filter: bool,
    pub audio_ctx_experimental: bool,
    pub screenshot: ScreenshotSettings,
    pub hotkeys: Hotkeys,
    pub output: OutputSettings,
    pub export: ExportSettings,
    pub copy: CopySettings,
    pub auto_copy_on_stop: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            locale: Locale::Ja,
            window: WindowSettings::default(),
            updates: UpdateSettings::default(),
            source: SourceSettings::default(),
            echo_guard: true,
            vad: VadSettings::default(),
            language: Language::Ja,
            model_id: crate::models::catalog::DEFAULT_MODEL_ID.into(),
            use_gpu: true,
            accuracy_first: false,
            partials: true,
            vocabulary: Vec::new(),
            hallucination_filter: true,
            audio_ctx_experimental: false,
            screenshot: ScreenshotSettings::default(),
            hotkeys: Hotkeys::default(),
            output: OutputSettings::default(),
            export: ExportSettings::default(),
            copy: CopySettings::default(),
            auto_copy_on_stop: false,
        }
    }
}

impl Settings {
    pub fn export_options(&self) -> crate::export::ExportOptions {
        crate::export::ExportOptions {
            timestamps: self.export.timestamps,
            labels: self.export.labels,
            merge_paragraphs: self.export.merge_paragraphs,
            screenshot_markers: self.copy.screenshot_markers,
        }
    }

    /// Clamps ranges and tidies lists after parsing.
    pub fn normalize(&mut self, default_root: &str) {
        let d = Settings::default();
        self.vad.start_threshold = if self.vad.start_threshold.is_finite() {
            self.vad.start_threshold.clamp(0.3, 0.8)
        } else {
            d.vad.start_threshold
        };
        self.vad.hangover_ms = self.vad.hangover_ms.clamp(400, 1200);
        let mut seen = std::collections::HashSet::new();
        self.vocabulary = self
            .vocabulary
            .iter()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty() && seen.insert(t.clone()))
            .take(MAX_VOCABULARY)
            .collect();
        for key in
            [&mut self.hotkeys.toggle, &mut self.hotkeys.screenshot, &mut self.hotkeys.mark, &mut self.hotkeys.cut]
        {
            *key = key.trim().to_string();
        }
        if crate::models::catalog::find(&self.model_id).is_none() {
            self.model_id = d.model_id;
        }
        if self.output.root.trim().is_empty() {
            self.output.root = default_root.to_string();
        }
        if self.output.title_template.trim().is_empty() {
            self.output.title_template = d.output.title_template;
        }
        if self.source.mic_device_id.as_deref().is_some_and(|s| s.trim().is_empty()) {
            self.source.mic_device_id = None;
        }
    }
}

fn same_json_type(a: &Value, b: &Value) -> bool {
    matches!(
        (a, b),
        (Value::Bool(_), Value::Bool(_))
            | (Value::Number(_), Value::Number(_))
            | (Value::String(_), Value::String(_))
            | (Value::Array(_), Value::Array(_))
            | (Value::Object(_), Value::Object(_))
    )
}

/// Overlays stored values on the defaults where the JSON type matches; unknown keys vanish.
fn overlay(defaults: &Value, stored: &Value) -> Value {
    match (defaults, stored) {
        (Value::Object(d), Value::Object(s)) => {
            let mut out = Map::new();
            for (k, dv) in d {
                let v = match s.get(k) {
                    Some(sv) => overlay(dv, sv),
                    None => dv.clone(),
                };
                out.insert(k.clone(), v);
            }
            Value::Object(out)
        }
        // Optional fields default to null: take any value, the typed parse decides.
        (Value::Null, sv) => sv.clone(),
        (dv, sv) if same_json_type(dv, sv) => sv.clone(),
        (dv, _) => dv.clone(),
    }
}

fn leaf_paths(v: &Value, prefix: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
    match v {
        Value::Object(m) if !m.is_empty() => {
            for (k, child) in m {
                prefix.push(k.clone());
                leaf_paths(child, prefix, out);
                prefix.pop();
            }
        }
        _ => out.push(prefix.clone()),
    }
}

fn get_path<'a>(v: &'a Value, path: &[String]) -> Option<&'a Value> {
    path.iter().try_fold(v, |cur, k| cur.get(k))
}

fn set_path(v: &mut Value, path: &[String], new: Value) {
    let mut cur = v;
    for k in &path[..path.len() - 1] {
        cur = cur.get_mut(k).expect("path exists in defaults");
    }
    cur[path.last().unwrap()] = new;
}

/// Parses stored settings leniently. Never fails.
pub fn parse_lenient(stored: &Value, default_root: &str) -> Settings {
    let defaults = serde_json::to_value(Settings::default()).expect("defaults serialize");
    let mut merged = overlay(&defaults, stored);
    if serde_json::from_value::<Settings>(merged.clone()).is_err() {
        // Revert every leaf that cannot be parsed on its own.
        let mut paths = Vec::new();
        leaf_paths(&defaults, &mut Vec::new(), &mut paths);
        for path in paths {
            let Some(value) = get_path(&merged, &path).cloned() else { continue };
            let mut candidate = defaults.clone();
            set_path(&mut candidate, &path, value);
            if serde_json::from_value::<Settings>(candidate).is_err() {
                let default_leaf = get_path(&defaults, &path).cloned().unwrap_or(Value::Null);
                set_path(&mut merged, &path, default_leaf);
            }
        }
    }
    let mut settings = serde_json::from_value::<Settings>(merged).unwrap_or_default();
    settings.normalize(default_root);
    settings
}

/// The UI language for stored settings that have none yet (the first launch): Japanese on a
/// Japanese Windows, English otherwise. A saved choice always wins.
pub fn initial_locale(stored: &Value, japanese_windows: bool) -> Option<Locale> {
    stored.get("locale").is_none().then_some(if japanese_windows { Locale::Ja } else { Locale::En })
}

/// Deep-merges a partial update (`set_settings`) into the current settings.
pub fn merge_partial(current: &Settings, partial: &Value, default_root: &str) -> Settings {
    fn deep_merge(base: &mut Value, patch: &Value) {
        match (base, patch) {
            (Value::Object(b), Value::Object(p)) => {
                for (k, pv) in p {
                    match b.get_mut(k) {
                        Some(bv) if bv.is_object() && pv.is_object() => deep_merge(bv, pv),
                        _ => {
                            b.insert(k.clone(), pv.clone());
                        }
                    }
                }
            }
            (b, p) => *b = p.clone(),
        }
    }
    let mut base = serde_json::to_value(current).expect("settings serialize");
    deep_merge(&mut base, partial);
    parse_lenient(&base, default_root)
}

/// Internal state that is not a user setting.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppStateFile {
    pub setup_done: bool,
    /// GPU init failed for this model and GPU name; retry when either changes.
    pub gpu_failure: Option<GpuFailure>,
    /// Set before a GPU init and cleared after; still set at launch means it crashed.
    pub gpu_init_in_progress: Option<GpuFailure>,
    pub last_update_check: Option<i64>,
    /// Speed tier per model from the benchmark.
    pub benchmarks: std::collections::BTreeMap<String, BenchmarkRecord>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuFailure {
    pub model_id: String,
    pub gpu_name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkRecord {
    pub seconds: f32,
    pub tier: String,
    pub gpu: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ROOT: &str = "C:\\Users\\x\\Documents\\Kikitori";

    #[test]
    fn defaults_match_spec() {
        let s = parse_lenient(&json!({}), ROOT);
        assert_eq!(s.locale, Locale::Ja);
        assert!(!s.window.always_on_top);
        assert_eq!(s.window.layout, Layout::Compact);
        assert_eq!(s.source.mode, SourceMode::System);
        assert!(s.source.include_mic);
        assert_eq!(s.vad.start_threshold, 0.5);
        assert_eq!(s.vad.hangover_ms, 640);
        assert_eq!(s.model_id, "turbo-q5");
        assert_eq!(s.hotkeys.toggle, "");
        assert_eq!(s.hotkeys.screenshot, "");
        assert_eq!(s.screenshot.target, ScreenshotTarget::AppWindow);
        assert_eq!(s.output.root, ROOT);
        assert_eq!(s.export.labels, LabelMode::Auto);
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["window"]["alwaysOnTop"], false);
        assert_eq!(v["screenshot"]["target"], "appWindow");
        assert_eq!(v["copy"]["screenshotMarkers"], true);
    }

    #[test]
    fn unknown_keys_dropped_and_bad_values_reset() {
        let stored = json!({
            "bogus": 1,
            "locale": "fr",
            "window": { "alwaysOnTop": false, "layout": "huge", "extra": true },
            "vad": { "startThreshold": 0.95, "hangoverMs": 100 },
            "vocabulary": ["大澤研", " ", "大澤研", "Kikitori"],
            "modelId": "nope",
            "language": "auto",
            "hotkeys": { "toggle": " Ctrl+Alt+K ", "screenshot": "" }
        });
        let s = parse_lenient(&stored, ROOT);
        assert_eq!(s.locale, Locale::Ja);
        assert!(!s.window.always_on_top);
        assert_eq!(s.window.layout, Layout::Compact);
        assert_eq!(s.vad.start_threshold, 0.8);
        assert_eq!(s.vad.hangover_ms, 400);
        assert_eq!(s.vocabulary, vec!["大澤研", "Kikitori"]);
        assert_eq!(s.model_id, "turbo-q5");
        assert_eq!(s.language, Language::Auto);
        assert_eq!(s.hotkeys.toggle, "Ctrl+Alt+K");
        assert_eq!(s.hotkeys.screenshot, "");
        let v = serde_json::to_value(&s).unwrap();
        assert!(v.get("bogus").is_none());
        assert!(v["window"].get("extra").is_none());
    }

    #[test]
    fn wrong_types_fall_back() {
        let s = parse_lenient(&json!({ "echoGuard": "yes", "vad": 3, "vocabulary": "x" }), ROOT);
        assert!(s.echo_guard);
        assert_eq!(s.vad, VadSettings::default());
        assert!(s.vocabulary.is_empty());
    }

    #[test]
    fn partial_update_merges_deeply() {
        let s = parse_lenient(&json!({}), ROOT);
        let s2 = merge_partial(
            &s,
            &json!({ "window": { "layout": "expanded" }, "source": { "micDeviceId": "dev1" } }),
            ROOT,
        );
        assert_eq!(s2.window.layout, Layout::Expanded);
        assert!(!s2.window.always_on_top);
        assert_eq!(s2.source.mic_device_id.as_deref(), Some("dev1"));
        let s3 = merge_partial(&s2, &json!({ "source": { "micDeviceId": null } }), ROOT);
        assert_eq!(s3.source.mic_device_id, None);
    }

    #[test]
    fn first_launch_follows_the_display_language() {
        assert_eq!(initial_locale(&json!({}), true), Some(Locale::Ja));
        assert_eq!(initial_locale(&json!({}), false), Some(Locale::En));
        assert_eq!(initial_locale(&json!({ "locale": "ja" }), false), None);
        assert_eq!(initial_locale(&json!({ "locale": "en" }), true), None);
    }

    #[test]
    fn vocabulary_capped_at_fifty() {
        let terms: Vec<String> = (0..80).map(|i| format!("t{i}")).collect();
        let s = parse_lenient(&json!({ "vocabulary": terms }), ROOT);
        assert_eq!(s.vocabulary.len(), MAX_VOCABULARY);
    }
}
