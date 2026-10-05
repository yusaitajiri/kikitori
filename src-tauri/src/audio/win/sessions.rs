//! The app picker (`AudioAppLister`, section 7): processes with audio sessions first, then
//! other apps that have windows.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use base64::Engine as _;
use wasapi::{DeviceEnumerator, Direction, SessionState};

use super::devices::run_in_mta;
use super::process::{self, ProcMap};
use crate::audio::source::{AudioApp, AudioAppLister, friendly_name};

pub struct WinAudioAppLister;

impl AudioAppLister for WinAudioAppLister {
    fn list(&self) -> anyhow::Result<Vec<AudioApp>> {
        run_in_mta(list_apps)
    }
}

struct SessionHit {
    active: bool,
}

/// PIDs with an audio session on any active render device, and whether each is playing.
fn session_pids() -> anyhow::Result<HashMap<u32, SessionHit>> {
    let enumerator = DeviceEnumerator::new()?;
    let devices = enumerator.get_device_collection(&Direction::Render)?;
    let mut out: HashMap<u32, SessionHit> = HashMap::new();
    for i in 0..devices.get_nbr_devices()? {
        let Ok(device) = devices.get_device_at_index(i) else { continue };
        let Ok(manager) = device.get_iaudiosessionmanager() else { continue };
        let Ok(sessions) = manager.get_audiosessionenumerator() else { continue };
        for s in 0..sessions.get_count().unwrap_or(0) {
            let Ok(control) = sessions.get_session(s) else { continue };
            let Ok(pid) = control.get_process_id() else { continue };
            let active = matches!(control.get_state(), Ok(SessionState::Active));
            let entry = out.entry(pid).or_insert(SessionHit { active: false });
            entry.active |= active;
        }
    }
    Ok(out)
}

fn excluded(pid: u32, name: &str) -> bool {
    pid == 0 || pid == std::process::id() || name.eq_ignore_ascii_case("audiodg.exe")
}

fn list_apps() -> anyhow::Result<Vec<AudioApp>> {
    let procs = process::snapshot();
    let sessions = session_pids()?;

    // Merge sessions by root PID.
    let mut roots: BTreeMap<u32, bool> = BTreeMap::new();
    for (pid, hit) in &sessions {
        let name = procs.get(pid).map(|p| p.name.as_str()).unwrap_or("");
        if excluded(*pid, name) || !procs.contains_key(pid) {
            continue;
        }
        let root = process::root_pid(&procs, *pid);
        *roots.entry(root).or_insert(false) |= hit.active;
    }

    let mut with_session: Vec<AudioApp> =
        roots.iter().filter_map(|(&root, &active)| describe(&procs, root, active, true)).collect();
    with_session.sort_by(|a, b| b.active.cmp(&a.active).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));

    // その他: visible top-level windows whose process has no session yet.
    let seen: HashSet<u32> = roots.keys().copied().collect();
    let mut others: Vec<AudioApp> = window_pids()
        .into_iter()
        .map(|pid| process::root_pid(&procs, pid))
        .collect::<HashSet<u32>>()
        .into_iter()
        .filter(|root| !seen.contains(root))
        .filter_map(|root| describe(&procs, root, false, false))
        .collect();
    others.sort_by_cached_key(|a| a.name.to_lowercase());
    // One entry per executable is enough there.
    let mut exes = HashSet::new();
    others.retain(|a| exes.insert(a.exe.to_lowercase()));

    with_session.extend(others);
    Ok(with_session)
}

fn describe(procs: &ProcMap, root: u32, active: bool, has_session: bool) -> Option<AudioApp> {
    let info = procs.get(&root)?;
    if excluded(root, &info.name) {
        return None;
    }
    let exe_path = info.exe.clone();
    let exe = exe_path.as_deref().and_then(|p| p.to_str()).map(str::to_string).unwrap_or_else(|| info.name.clone());
    let name = friendly_name(&info.name)
        .map(str::to_string)
        .or_else(|| exe_path.as_deref().and_then(file_description))
        .unwrap_or_else(|| info.name.trim_end_matches(".exe").to_string());
    let icon_data_url = exe_path.as_deref().and_then(icon_data_url);
    Some(AudioApp { root_pid: root, exe, name, icon_data_url, active, has_session })
}

fn to_wide(s: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    s.encode_wide().chain(std::iter::once(0)).collect()
}

/// The executable's FileDescription from its version resource.
pub fn file_description(exe: &Path) -> Option<String> {
    use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
    use windows::core::PCWSTR;
    let wide = to_wide(exe.as_os_str());
    // SAFETY: buffers are sized from GetFileVersionInfoSizeW and outlive every pointer into them.
    unsafe {
        let size = GetFileVersionInfoSizeW(PCWSTR(wide.as_ptr()), None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(PCWSTR(wide.as_ptr()), None, size, data.as_mut_ptr().cast()).ok()?;
        let mut ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        let translation = to_wide(std::ffi::OsStr::new("\\VarFileInfo\\Translation"));
        let mut lang_cp = (0x0409u16, 0x04B0u16);
        if VerQueryValueW(data.as_ptr().cast(), PCWSTR(translation.as_ptr()), &mut ptr, &mut len).as_bool() && len >= 4
        {
            let pair = ptr as *const u16;
            lang_cp = (*pair, *pair.add(1));
        }
        let key = format!("\\StringFileInfo\\{:04x}{:04x}\\FileDescription", lang_cp.0, lang_cp.1);
        let key = to_wide(std::ffi::OsStr::new(&key));
        if !VerQueryValueW(data.as_ptr().cast(), PCWSTR(key.as_ptr()), &mut ptr, &mut len).as_bool() || len == 0 {
            return None;
        }
        let slice = std::slice::from_raw_parts(ptr as *const u16, len as usize);
        let end = slice.iter().position(|&c| c == 0).unwrap_or(slice.len());
        let s = String::from_utf16_lossy(&slice[..end]).trim().to_string();
        (!s.is_empty()).then_some(s)
    }
}

/// The executable's 32 px icon as a PNG data URL.
pub fn icon_data_url(exe: &Path) -> Option<String> {
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC, GetDIBits, GetObjectW,
        HGDIOBJ, ReleaseDC,
    };
    use windows::Win32::UI::Shell::ExtractIconExW;
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};
    use windows::core::PCWSTR;

    let wide = to_wide(exe.as_os_str());
    // SAFETY: every handle is checked and released; buffers are sized from the bitmap.
    unsafe {
        let mut large = HICON::default();
        if ExtractIconExW(PCWSTR(wide.as_ptr()), 0, Some(&mut large), None, 1) == 0 || large.is_invalid() {
            return None;
        }
        let mut info = ICONINFO::default();
        let got = GetIconInfo(large, &mut info).is_ok();
        let result = (|| {
            if !got || info.hbmColor.is_invalid() {
                return None;
            }
            let mut bmp = BITMAP::default();
            if GetObjectW(
                HGDIOBJ(info.hbmColor.0),
                std::mem::size_of::<BITMAP>() as i32,
                Some((&mut bmp as *mut BITMAP).cast()),
            ) == 0
            {
                return None;
            }
            let (w, h) = (bmp.bmWidth, bmp.bmHeight.abs());
            if w <= 0 || h <= 0 || w > 256 || h > 256 {
                return None;
            }
            let read = |bitmap| -> Option<Vec<u8>> {
                let mut bi = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: w,
                        biHeight: -h,
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB.0,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let mut buf = vec![0u8; (w * h * 4) as usize];
                let hdc = GetDC(None);
                let lines = GetDIBits(hdc, bitmap, 0, h as u32, Some(buf.as_mut_ptr().cast()), &mut bi, DIB_RGB_COLORS);
                ReleaseDC(None, hdc);
                (lines != 0).then_some(buf)
            };
            let mut bgra = read(info.hbmColor)?;
            if bgra.as_chunks::<4>().0.iter().all(|p| p[3] == 0) {
                // Old-style icon without alpha: take transparency from the mask.
                if let Some(mask) = read(info.hbmMask) {
                    for (px, m) in bgra.as_chunks_mut::<4>().0.iter_mut().zip(mask.as_chunks::<4>().0) {
                        px[3] = if m[0] == 0 { 255 } else { 0 };
                    }
                }
            }
            for px in bgra.as_chunks_mut::<4>().0 {
                px.swap(0, 2);
            }
            let img = image::RgbaImage::from_raw(w as u32, h as u32, bgra)?;
            let img = if w != 32 {
                image::imageops::resize(&img, 32, 32, image::imageops::FilterType::Triangle)
            } else {
                img
            };
            let mut png = Vec::new();
            img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
            Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)))
        })();
        if !info.hbmColor.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(info.hbmColor.0));
        }
        if !info.hbmMask.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(info.hbmMask.0));
        }
        let _ = DestroyIcon(large);
        result
    }
}

/// PIDs that own a visible, titled, non-tool, non-cloaked top-level window.
pub fn window_pids() -> Vec<u32> {
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GWL_EXSTYLE, GetWindowLongW, GetWindowTextLengthW, GetWindowThreadProcessId, IsWindowVisible,
        WS_EX_TOOLWINDOW,
    };
    use windows::core::BOOL;

    unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: lparam is the &mut Vec<u32> passed below, alive for the EnumWindows call.
        unsafe {
            let out = &mut *(lparam.0 as *mut Vec<u32>);
            if !IsWindowVisible(hwnd).as_bool() || GetWindowTextLengthW(hwnd) == 0 {
                return BOOL(1);
            }
            if (GetWindowLongW(hwnd, GWL_EXSTYLE) as u32) & WS_EX_TOOLWINDOW.0 != 0 {
                return BOOL(1);
            }
            let mut cloaked = 0u32;
            if DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, (&mut cloaked as *mut u32).cast(), 4).is_ok() && cloaked != 0
            {
                return BOOL(1);
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid != 0 {
                out.push(pid);
            }
            BOOL(1)
        }
    }

    let mut pids: Vec<u32> = Vec::new();
    // SAFETY: the callback only touches `pids` through lparam during this call.
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut pids as *mut Vec<u32> as isize));
    }
    pids.sort_unstable();
    pids.dedup();
    pids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explorer_has_description_and_icon() {
        let explorer = Path::new("C:\\Windows\\explorer.exe");
        let desc = file_description(explorer);
        assert!(desc.is_some_and(|d| !d.is_empty()));
        let icon = icon_data_url(explorer).expect("icon");
        assert!(icon.starts_with("data:image/png;base64,"));
    }

    #[test]
    fn listing_apps_works() {
        let apps = WinAudioAppLister.list().unwrap();
        // Sessions (if any) come before window-only apps.
        let first_window_only = apps.iter().position(|a| !a.has_session).unwrap_or(apps.len());
        assert!(apps[first_window_only..].iter().all(|a| !a.has_session));
        assert!(apps.iter().all(|a| a.root_pid != std::process::id()));
    }
}
