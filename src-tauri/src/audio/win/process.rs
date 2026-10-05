//! Process tree helpers for the app picker and reattach (section 7).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

#[derive(Debug, Clone, PartialEq)]
pub struct ProcInfo {
    pub pid: u32,
    pub parent: Option<u32>,
    pub name: String,
    pub exe: Option<PathBuf>,
}

pub type ProcMap = HashMap<u32, ProcInfo>;

pub fn snapshot() -> ProcMap {
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet),
    );
    sys.processes()
        .iter()
        .map(|(pid, p)| {
            let pid = pid.as_u32();
            (
                pid,
                ProcInfo {
                    pid,
                    parent: p.parent().map(|pp| pp.as_u32()),
                    name: p.name().to_string_lossy().into_owned(),
                    exe: p.exe().map(Path::to_path_buf),
                },
            )
        })
        .collect()
}

/// Walks up while the parent has the same executable name. Chrome and Edge play audio from
/// a child utility process; the root covers every tab.
pub fn root_pid(procs: &ProcMap, pid: u32) -> u32 {
    let mut current = pid;
    let mut guard = 0;
    while let Some(info) = procs.get(&current) {
        guard += 1;
        if guard > 64 {
            break;
        }
        let Some(parent) = info.parent.and_then(|pp| procs.get(&pp)) else { break };
        if parent.pid == current || !parent.name.eq_ignore_ascii_case(&info.name) {
            break;
        }
        current = parent.pid;
    }
    current
}

/// Every process whose root is `root`.
pub fn tree_of(procs: &ProcMap, root: u32) -> Vec<u32> {
    procs.keys().copied().filter(|&pid| root_pid(procs, pid) == root).collect()
}

/// A running root process with this executable path (for reattaching, FR-14).
pub fn find_root_by_exe(procs: &ProcMap, exe: &Path) -> Option<u32> {
    let mut roots: Vec<u32> = procs
        .values()
        .filter(|p| p.exe.as_deref().is_some_and(|e| same_path(e, exe)))
        .map(|p| root_pid(procs, p.pid))
        .collect();
    roots.sort_unstable();
    roots.dedup();
    roots.into_iter().next()
}

pub fn same_path(a: &Path, b: &Path) -> bool {
    a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: u32, parent: Option<u32>, name: &str) -> (u32, ProcInfo) {
        (pid, ProcInfo { pid, parent, name: name.into(), exe: Some(PathBuf::from(format!("C:\\Apps\\{name}"))) })
    }

    fn sample() -> ProcMap {
        [
            p(1, None, "explorer.exe"),
            p(10, Some(1), "chrome.exe"),
            p(11, Some(10), "chrome.exe"),
            p(12, Some(11), "chrome.exe"),
            p(20, Some(1), "Zoom.exe"),
            p(21, Some(20), "CptHost.exe"),
        ]
        .into_iter()
        .collect()
    }

    #[test]
    fn root_walks_same_name_parents() {
        let procs = sample();
        assert_eq!(root_pid(&procs, 12), 10);
        assert_eq!(root_pid(&procs, 10), 10);
        assert_eq!(root_pid(&procs, 21), 21);
        assert_eq!(root_pid(&procs, 999), 999);
    }

    #[test]
    fn tree_and_find_by_exe() {
        let procs = sample();
        let mut tree = tree_of(&procs, 10);
        tree.sort();
        assert_eq!(tree, vec![10, 11, 12]);
        assert_eq!(find_root_by_exe(&procs, Path::new("c:\\apps\\chrome.exe")), Some(10));
        assert_eq!(find_root_by_exe(&procs, Path::new("C:\\Apps\\nope.exe")), None);
    }

    #[test]
    fn cycles_do_not_hang() {
        let procs: ProcMap = [p(5, Some(6), "a.exe"), p(6, Some(5), "a.exe")].into_iter().collect();
        let _ = root_pid(&procs, 5);
    }

    #[test]
    fn real_snapshot_contains_us() {
        let procs = snapshot();
        assert!(procs.contains_key(&std::process::id()));
    }
}
