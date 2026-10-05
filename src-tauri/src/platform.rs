//! Small OS helpers: the QPC clock, OS build, display language, thread priority, cursor position.

/// QueryPerformanceCounter in 100 ns units, the same base WASAPI uses for packet timestamps.
#[cfg(windows)]
pub fn qpc_now_100ns() -> u64 {
    use std::sync::OnceLock;
    use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
    static FREQ: OnceLock<i64> = OnceLock::new();
    let freq = *FREQ.get_or_init(|| {
        let mut f = 0i64;
        // SAFETY: plain out-parameter.
        unsafe { QueryPerformanceFrequency(&mut f) }.ok();
        f.max(1)
    });
    let mut c = 0i64;
    // SAFETY: plain out-parameter.
    unsafe { QueryPerformanceCounter(&mut c) }.ok();
    ((c as i128) * 10_000_000 / freq as i128) as u64
}

#[cfg(not(windows))]
pub fn qpc_now_100ns() -> u64 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static START: OnceLock<Instant> = OnceLock::new();
    (START.get_or_init(Instant::now).elapsed().as_nanos() / 100) as u64
}

/// Windows build number (e.g. 26100), via RtlGetVersion so the manifest cannot hide it.
#[cfg(windows)]
pub fn os_build() -> u32 {
    use windows::Wdk::System::SystemServices::RtlGetVersion;
    use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
    let mut info =
        OSVERSIONINFOW { dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32, ..Default::default() };
    // SAFETY: `info` is a properly sized OSVERSIONINFOW.
    let status = unsafe { RtlGetVersion(&mut info) };
    if status.is_ok() { info.dwBuildNumber } else { 0 }
}

#[cfg(not(windows))]
pub fn os_build() -> u32 {
    0
}

/// Whether Windows' display language (the user's UI language) is Japanese.
#[cfg(windows)]
pub fn display_language_is_japanese() -> bool {
    const LANG_JAPANESE: u16 = 0x11;
    // SAFETY: takes no arguments and only returns a language ID.
    let lang = unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() };
    lang & 0x3ff == LANG_JAPANESE
}

#[cfg(not(windows))]
pub fn display_language_is_japanese() -> bool {
    true
}

/// Per-app capture needs build 20348 or later (in practice Windows 11).
pub const APP_LOOPBACK_MIN_BUILD: u32 = 20348;

pub fn app_loopback_supported() -> bool {
    cfg!(windows) && os_build() >= APP_LOOPBACK_MIN_BUILD
}

/// Raises the calling thread to MMCSS "Pro Audio". Keep the guard alive for the thread.
#[cfg(windows)]
pub struct MmcssGuard(windows::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for MmcssGuard {
    fn drop(&mut self) {
        use windows::Win32::System::Threading::AvRevertMmThreadCharacteristics;
        // SAFETY: the handle came from AvSetMmThreadCharacteristicsW on this thread.
        let _ = unsafe { AvRevertMmThreadCharacteristics(self.0) };
    }
}

#[cfg(windows)]
pub fn raise_to_pro_audio() -> Option<MmcssGuard> {
    use windows::Win32::System::Threading::AvSetMmThreadCharacteristicsW;
    use windows::core::w;
    let mut index = 0u32;
    // SAFETY: valid task name and out-parameter.
    match unsafe { AvSetMmThreadCharacteristicsW(w!("Pro Audio"), &mut index) } {
        Ok(h) => Some(MmcssGuard(h)),
        Err(err) => {
            tracing::warn!("MMCSS Pro Audio failed: {err}");
            None
        }
    }
}

/// Cursor position in physical pixels.
#[cfg(windows)]
pub fn cursor_pos() -> Option<(i32, i32)> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut p = POINT::default();
    // SAFETY: plain out-parameter.
    unsafe { GetCursorPos(&mut p) }.ok()?;
    Some((p.x, p.y))
}

/// True while a process with this PID exists and has not exited.
#[cfg(windows)]
pub fn process_alive(pid: u32) -> bool {
    use windows::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    // SAFETY: standard handle lifecycle; closed below.
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return false };
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code).is_ok();
        let _ = CloseHandle(h);
        ok && code == STILL_ACTIVE.0 as u32
    }
}

/// Plays a WAV file from memory without waiting. A newer sound cuts off one still playing.
#[cfg(windows)]
pub fn play_wav(wav: &'static [u8]) {
    use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
    use windows::core::PCWSTR;
    // SAFETY: with SND_MEMORY the pointer is the WAV data, which is 'static, so it outlives the
    // asynchronous playback. A sound that fails to play is not worth reporting.
    let _ = unsafe { PlaySoundW(PCWSTR(wav.as_ptr().cast()), None, SND_MEMORY | SND_ASYNC | SND_NODEFAULT) };
}

#[cfg(not(windows))]
pub fn play_wav(_wav: &'static [u8]) {}

/// Total physical memory in bytes.
pub fn total_memory() -> u64 {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    sys.total_memory()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qpc_is_monotonic_and_in_100ns_units() {
        let a = qpc_now_100ns();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let b = qpc_now_100ns();
        let elapsed_ms = (b - a) / 10_000;
        assert!((15..200).contains(&elapsed_ms), "{elapsed_ms}");
    }

    #[cfg(windows)]
    #[test]
    fn os_build_is_reported() {
        assert!(os_build() > 10_000);
    }

    #[cfg(windows)]
    #[test]
    fn own_process_is_alive() {
        assert!(process_alive(std::process::id()));
        assert!(!process_alive(0xFFFF_FFF0));
    }
}
