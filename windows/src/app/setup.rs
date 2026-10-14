//! "Set up the service…": runs install.ps1 with administrator rights (Windows asks first), which
//! installs the app for everyone with its helper service and WinDivert.

use super::App;
use std::path::PathBuf;
use std::rc::Rc;
use windows::core::{w, HSTRING};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// install.ps1: next to the program once installed, or in windows\ for a build of it.
fn script() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let here = exe.parent()?;
    [here.join("install.ps1"), here.join(r"..\..\install.ps1")].into_iter().find(|p| p.exists())
}

pub fn run(app: &Rc<App>) {
    let Some(script) = script() else {
        return app.say("install.ps1 isn't next to the program: run it from the hotspot-manager-linux folder", true);
    };
    // (-Elevated: it has the rights already, from this request)
    let args = format!("-NoProfile -ExecutionPolicy Bypass -File \"{}\" -Elevated", script.display());
    // SAFETY: asks Windows to run PowerShell as administrator (it shows its own prompt)
    let result = unsafe {
        ShellExecuteW(Some(app.window.hwnd()), w!("runas"), w!("powershell.exe"), &HSTRING::from(args), None, SW_SHOWNORMAL)
    };
    if result.0 as isize > 32 {
        app.say("Installing the service: follow the window that opened.", false);
    } else {
        app.say("The service wasn't installed (Windows' permission was turned down?)", true);
    }
}
