//! The helper's log: the last few hundred lines for the window's "View log", and a file
//! (%ProgramData%\Hotspot Manager\helper.log).

use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use windows::Win32::System::SystemInformation::GetLocalTime;

static LINES: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
const KEEP: usize = 400;

/// Where the helper keeps its files (`HOTSPOT_HOME` for trying a build).
pub fn folder() -> PathBuf {
    let dir = match std::env::var_os("HOTSPOT_HOME") {
        Some(home) => PathBuf::from(home),
        None => PathBuf::from(std::env::var_os("ProgramData").unwrap_or_else(|| r"C:\ProgramData".into()))
            .join("Hotspot Manager"),
    };
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn line(text: impl AsRef<str>) {
    // SAFETY: plain query
    let t = unsafe { GetLocalTime() };
    let line = format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}  {}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute,
                       t.wSecond, text.as_ref());
    let mut lines = LINES.lock().unwrap();
    lines.push_back(line.clone());
    while lines.len() > KEEP {
        lines.pop_front();
    }
    drop(lines);
    let file = folder().join("helper.log");
    if std::fs::metadata(&file).is_ok_and(|m| m.len() > 1 << 20) {
        let _ = std::fs::rename(&file, folder().join("helper.old.log"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&file) {
        let _ = writeln!(f, "{line}");
    }
}

pub fn lines() -> Vec<String> {
    LINES.lock().unwrap().iter().cloned().collect()
}
