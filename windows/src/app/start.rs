//! Starting the app: its window and parts, the message loop, and running work from other
//! threads on the UI thread (`post`).

use super::{devices, settings, top, App, HEIGHT, PAGE, TOP, WIDTH};
use crate::store::Store;
use crate::theme;
use crate::wnd::{self, Window};
use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{mpsc, Mutex, OnceLock};
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Controls::{InitCommonControlsEx, ICC_STANDARD_CLASSES, ICC_WIN95_CLASSES, INITCOMMONCONTROLSEX};
use windows::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;
use windows::Win32::UI::WindowsAndMessaging::*;

thread_local! {
    static APP: OnceCell<Rc<App>> = const { OnceCell::new() };
    static INBOX: OnceCell<mpsc::Receiver<Job>> = const { OnceCell::new() };
}

pub fn app() -> Rc<App> {
    APP.with(|app| app.get().expect("started").clone())
}

/// The app, once it's made (paints may come before).
pub(super) fn started() -> Option<Rc<App>> {
    APP.with(|app| app.get().cloned())
}

type Job = Box<dyn FnOnce(&Rc<App>) + Send>;
static OUTBOX: OnceLock<Mutex<mpsc::Sender<Job>>> = OnceLock::new();
static WINDOW: AtomicIsize = AtomicIsize::new(0);
const WM_WAKE: u32 = WM_APP + 1;
const TICK: usize = 1;

/// Run `job` on the UI thread, from any thread.
pub fn post(job: impl FnOnce(&Rc<App>) + Send + 'static) {
    if let Some(outbox) = OUTBOX.get() {
        let _ = outbox.lock().unwrap().send(Box::new(job));
        let hwnd = HWND(WINDOW.load(Ordering::SeqCst) as *mut _);
        // SAFETY: posting to our window from any thread is what PostMessage is for
        unsafe { let _ = PostMessageW(Some(hwnd), WM_WAKE, WPARAM(0), LPARAM(0)); }
    }
}

pub fn run() -> i32 {
    // SAFETY: looking for our own window; bringing it forward
    unsafe {
        if let Ok(open) = FindWindowW(wnd::CLASS, w!("Hotspot Manager")) {
            let _ = ShowWindow(open, SW_RESTORE);
            let _ = SetForegroundWindow(open);
            return 0;
        }
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED); // Windows' dialogs
        let classes = INITCOMMONCONTROLSEX { dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
                                             dwICC: ICC_WIN95_CLASSES | ICC_STANDARD_CLASSES };
        let _ = InitCommonControlsEx(&classes);
    }
    let (outbox, inbox) = mpsc::channel();
    let _ = OUTBOX.set(Mutex::new(outbox));
    INBOX.with(|i| { let _ = i.set(inbox); });
    let window = Window::dialog("Hotspot Manager", None, (WIDTH, HEIGHT), true);
    WINDOW.store(window.hwnd().0 as isize, Ordering::SeqCst);
    let top = top::Top::new(&window);
    let tabs = window.tabs(&["Devices", "Settings"], |page| app().show_page(page));
    wnd::place(tabs, 0, TOP + 1, WIDTH, wnd::tabs::TABS);
    let devices = devices::Devices::new(&window);
    let settings = settings::Settings::new(&window);
    window.set_surfaces(vec![(wnd::rect(0, 0, WIDTH, TOP), theme::VIEW), (wnd::rect(0, TOP, WIDTH, 1), theme::BORDER),
                             (wnd::rect(0, PAGE, WIDTH, HEIGHT - PAGE), theme::VIEW)]);
    let this = Rc::new(App {
        window: window.clone(), store: RefCell::new(Store::load()), status: RefCell::default(), busy: Cell::default(),
        note: RefCell::default(), speed: Cell::default(), traffic: Cell::default(), asking: Cell::new(false),
        ticks: Cell::new(0), top, devices, settings,
    });
    APP.with(|a| { let _ = a.set(this.clone()); });
    window.hook(|msg, wparam, _| match msg {
        WM_WAKE => {
            let jobs: Vec<Job> = INBOX.with(|inbox| inbox.get().map(|rx| rx.try_iter().collect()).unwrap_or_default());
            let app = app();
            for job in jobs {
                job(&app);
            }
            Some(LRESULT(0))
        }
        WM_TIMER if wparam.0 == TICK => {
            app().tick();
            Some(LRESULT(0))
        }
        _ => None,
    });
    window.on_close(|| {
        // SAFETY: ends our message loop (the hotspot stays as it is)
        unsafe { PostQuitMessage(0) };
        true
    });
    this.show_page(0);
    this.settings.start();
    this.update();
    wnd::set_client_height(window.hwnd(), HEIGHT);
    wnd::center(window.hwnd());
    window.show();
    // SAFETY: a timer on our window
    unsafe { SetTimer(Some(window.hwnd()), TICK, 1000, None) };
    this.refresh();
    message_loop();
    theme::stop();
    0
}

fn message_loop() {
    let mut msg = MSG::default();
    // SAFETY: the standard loop on this thread's messages
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let active = GetActiveWindow();
            if !active.is_invalid() && IsDialogMessageW(active, &msg).as_bool() {
                continue; // Tab, Enter and Escape in dialogs
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

