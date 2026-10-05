//! OS-neutral audio source interface (section 6). Windows code lives in `audio::win`.

use crossbeam_channel::Sender;
use serde::Serialize;

use crate::session::model::SourceId;

/// One capture packet, placed on the timeline by its QPC timestamp.
#[derive(Debug, Clone)]
pub struct RawChunk {
    pub source: SourceId,
    /// QPC time of the first frame, in 100 ns units.
    pub qpc_100ns: u64,
    pub rate: u32,
    pub channels: u16,
    /// Interleaved samples in [-1, 1].
    pub samples: Vec<f32>,
    /// The buffer was flagged silent; treat it as zeros of the same length.
    pub silent_flag: bool,
}

/// Something a capture thread wants the recorder to know.
#[derive(Debug, Clone, PartialEq)]
pub enum SourceStatus {
    Started {
        source: SourceId,
        rate: u32,
        channels: u16,
        device: Option<String>,
    },
    /// The device or app went away; capture keeps trying.
    Lost {
        source: SourceId,
    },
    /// The app came back after exiting (FR-14).
    Reattached {
        source: SourceId,
        detail: String,
    },
    /// The stream reopened on another device: the default changed (FR-15) or the device
    /// went away and came back.
    DeviceSwitched {
        source: SourceId,
        device: Option<String>,
    },
    /// The mic is blocked by Windows privacy settings.
    MicDenied,
    /// App loopback delivers only zeros while the app's meter shows sound.
    AppSilent,
    /// The stream stopped delivering packets and a restart did not help.
    Failed {
        source: SourceId,
        message: String,
    },
}

pub trait AudioSource: Send {
    fn id(&self) -> SourceId;
    /// Starts capture; returns once the stream is running (or failed to open).
    fn start(&mut self, tx: Sender<RawChunk>) -> anyhow::Result<()>;
    fn stop(&mut self);
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioApp {
    pub root_pid: u32,
    pub exe: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_data_url: Option<String>,
    /// Playing audio right now.
    pub active: bool,
    /// Has an audio session at all (otherwise listed under その他 by its window).
    pub has_session: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

pub trait AudioAppLister {
    fn list(&self) -> anyhow::Result<Vec<AudioApp>>;
}

/// Friendly names for common apps (section 7).
pub fn friendly_name(exe: &str) -> Option<&'static str> {
    match exe.to_ascii_lowercase().as_str() {
        "zoom.exe" => Some("Zoom"),
        "ms-teams.exe" | "teams.exe" => Some("Microsoft Teams"),
        "chrome.exe" => Some("Google Chrome"),
        "msedge.exe" => Some("Microsoft Edge"),
        "firefox.exe" => Some("Firefox"),
        "discord.exe" => Some("Discord"),
        "slack.exe" => Some("Slack"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn friendly_names() {
        assert_eq!(super::friendly_name("Zoom.exe"), Some("Zoom"));
        assert_eq!(super::friendly_name("ms-teams.exe"), Some("Microsoft Teams"));
        assert_eq!(super::friendly_name("notepad.exe"), None);
    }
}
