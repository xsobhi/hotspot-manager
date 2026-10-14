//! One thread for Windows' hotspot calls, which wait for Windows (WinRT, multithreaded): they
//! run there one after another, and send their answers back to the window (app::post).

use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

type Job = Box<dyn FnOnce() + Send>;

pub fn run(job: impl FnOnce() + Send + 'static) {
    static JOBS: OnceLock<Mutex<Sender<Job>>> = OnceLock::new();
    let jobs = JOBS.get_or_init(|| {
        let (sender, jobs) = channel::<Job>();
        std::thread::spawn(move || {
            // SAFETY: once, at the start of this thread
            unsafe { let _ = RoInitialize(RO_INIT_MULTITHREADED); }
            for job in jobs {
                job();
            }
        });
        Mutex::new(sender)
    });
    let _ = jobs.lock().unwrap().send(Box::new(job));
}
