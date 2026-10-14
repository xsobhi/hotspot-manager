//! The helper as a Windows service ("HotspotManager", started with Windows), which is how it has
//! administrator rights without asking each time: Windows' Service Control Manager runs
//! `hotspot-manager --service` and tells it when to stop.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::NO_ERROR;
use windows::Win32::System::Services::*;

pub const NAME: PCWSTR = w!("HotspotManager");
static STATUS: AtomicIsize = AtomicIsize::new(0);
static STOP: AtomicBool = AtomicBool::new(false);

pub fn run() -> i32 {
    let table = [
        SERVICE_TABLE_ENTRYW { lpServiceName: PWSTR(NAME.as_ptr() as *mut _), lpServiceProc: Some(main) },
        SERVICE_TABLE_ENTRYW::default(),
    ];
    // SAFETY: hands this thread to the Service Control Manager until the service ends
    match unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) } {
        Ok(()) => 0,
        Err(_) => 1, // not started by Windows as a service
    }
}

fn report(state: SERVICE_STATUS_CURRENT_STATE) {
    let accepts = if state == SERVICE_RUNNING { SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN } else { 0 };
    let status = SERVICE_STATUS { dwServiceType: SERVICE_WIN32_OWN_PROCESS, dwCurrentState: state,
                                  dwControlsAccepted: accepts, dwWaitHint: 5000, ..Default::default() };
    // SAFETY: our service's status handle, from RegisterServiceCtrlHandlerExW
    unsafe { let _ = SetServiceStatus(SERVICE_STATUS_HANDLE(STATUS.load(Ordering::SeqCst) as *mut _), &status); }
}

unsafe extern "system" fn main(_count: u32, _args: *mut PWSTR) {
    // SAFETY: registers our handler for this service
    let Ok(handle) = (unsafe { RegisterServiceCtrlHandlerExW(NAME, Some(control), None) }) else { return };
    STATUS.store(handle.0 as isize, Ordering::SeqCst);
    report(SERVICE_RUNNING);
    super::run(&STOP);
    report(SERVICE_STOPPED);
}

unsafe extern "system" fn control(code: u32, _kind: u32, _data: *mut c_void, _context: *mut c_void) -> u32 {
    if code == SERVICE_CONTROL_STOP || code == SERVICE_CONTROL_SHUTDOWN {
        STOP.store(true, Ordering::SeqCst);
        report(SERVICE_STOP_PENDING);
    }
    NO_ERROR.0
}
