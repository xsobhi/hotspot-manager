//! WinDivert (https://reqrypt.org/windivert.html, LGPL): a signed Windows driver that hands
//! network packets to a program and takes them back, which is how the helper counts, slows down
//! and stops devices' traffic. Loaded from WinDivert.dll next to the program, if it's there.

use std::ffi::{c_void, CString};
use std::sync::OnceLock;
use windows::core::{s, w, PCSTR};
use windows::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_APPLICATION_DIR};

pub const LAYER_NETWORK: u32 = 0;
pub const LAYER_FORWARD: u32 = 1;
const PARAM_QUEUE_LENGTH: u32 = 0;
const PARAM_QUEUE_TIME: u32 = 1;
const PARAM_QUEUE_SIZE: u32 = 2;

/// WINDIVERT_ADDRESS: where a packet was seen (80 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Address {
    pub timestamp: i64,
    pub flags: u32, // Layer:8 Event:8 Sniffed:1 Outbound:1 Loopback:1 Impostor:1 IPv6:1 checksums:3
    reserved: u32,
    pub if_idx: u32, // (network layers)
    pub sub_if_idx: u32,
    rest: [u8; 56],
}

impl Default for Address {
    fn default() -> Address {
        Address { timestamp: 0, flags: 0, reserved: 0, if_idx: 0, sub_if_idx: 0, rest: [0; 56] }
    }
}

impl Address {
    pub fn outbound(&self) -> bool {
        self.flags & (1 << 17) != 0
    }

    pub fn set_outbound(&mut self, on: bool) {
        self.flags = if on { self.flags | (1 << 17) } else { self.flags & !(1 << 17) };
    }

    pub fn set_layer(&mut self, layer: u32) {
        self.flags = (self.flags & !0xff) | (layer & 0xff);
    }
}

type Open = unsafe extern "C" fn(*const i8, u32, i16, u64) -> HANDLE;
type Recv = unsafe extern "C" fn(HANDLE, *mut c_void, u32, *mut u32, *mut Address) -> i32;
type Send = unsafe extern "C" fn(HANDLE, *const c_void, u32, *mut u32, *const Address) -> i32;
type SetParam = unsafe extern "C" fn(HANDLE, u32, u64) -> i32;
type Close = unsafe extern "C" fn(HANDLE) -> i32;
type Shutdown = unsafe extern "C" fn(HANDLE, u32) -> i32;
type Checksums = unsafe extern "C" fn(*mut c_void, u32, *mut Address, u64) -> i32;

struct Api {
    open: Open,
    recv: Recv,
    send: Send,
    set_param: SetParam,
    close: Close,
    shutdown: Shutdown,
    checksums: Checksums,
}

/// WinDivert's functions, or why they can't be used.
fn api() -> Result<&'static Api, String> {
    static API: OnceLock<Result<Api, String>> = OnceLock::new();
    API.get_or_init(|| {
        // SAFETY: loads WinDivert.dll from the program's own folder and looks up its functions,
        // whose signatures are WinDivert 2's
        unsafe {
            let dll = LoadLibraryExW(w!("WinDivert.dll"), None, LOAD_LIBRARY_SEARCH_APPLICATION_DIR)
                .map_err(|_| "WinDivert isn't installed next to the helper (see windows\\install.ps1)".to_string())?;
            let find = |name: PCSTR| GetProcAddress(dll, name).ok_or_else(|| "WinDivert.dll is too old".to_string());
            Ok(Api {
                open: std::mem::transmute::<_, Open>(find(s!("WinDivertOpen"))?),
                recv: std::mem::transmute::<_, Recv>(find(s!("WinDivertRecv"))?),
                send: std::mem::transmute::<_, Send>(find(s!("WinDivertSend"))?),
                set_param: std::mem::transmute::<_, SetParam>(find(s!("WinDivertSetParam"))?),
                close: std::mem::transmute::<_, Close>(find(s!("WinDivertClose"))?),
                shutdown: std::mem::transmute::<_, Shutdown>(find(s!("WinDivertShutdown"))?),
                checksums: std::mem::transmute::<_, Checksums>(find(s!("WinDivertHelperCalcChecksums"))?),
            })
        }
    }).as_ref().map_err(Clone::clone)
}

/// A WinDivert handle: the packets matching its filter come to us instead of going on.
pub struct Divert {
    handle: HANDLE,
}

// SAFETY: WinDivert handles are made for receiving and sending from several threads
unsafe impl std::marker::Send for Divert {}
unsafe impl Sync for Divert {}

impl Divert {
    pub fn open(filter: &str, layer: u32, priority: i16) -> Result<Divert, String> {
        Divert::with_flags(filter, layer, priority, 0)
    }

    /// Only watch the matching packets (they go on as usual).
    pub fn sniff(filter: &str, layer: u32) -> Result<Divert, String> {
        Divert::with_flags(filter, layer, 0, 1) // WINDIVERT_FLAG_SNIFF
    }

    fn with_flags(filter: &str, layer: u32, priority: i16, flags: u64) -> Result<Divert, String> {
        let api = api()?;
        let filter = CString::new(filter).map_err(|e| e.to_string())?;
        // SAFETY: a filter string that outlives the call
        let handle = unsafe { (api.open)(filter.as_ptr(), layer, priority, flags) };
        if handle == INVALID_HANDLE_VALUE || handle.is_invalid() {
            return Err(format!("WinDivert couldn't start ({})", windows::core::Error::from_win32().message()));
        }
        // SAFETY: our handle; a bigger queue, so packets aren't lost while we're busy
        unsafe {
            (api.set_param)(handle, PARAM_QUEUE_LENGTH, 16384);
            (api.set_param)(handle, PARAM_QUEUE_TIME, 4000);
            (api.set_param)(handle, PARAM_QUEUE_SIZE, 32 << 20);
        }
        Ok(Divert { handle })
    }

    /// The next packet (waits for it); None when the handle is closed.
    pub fn recv(&self, buffer: &mut [u8]) -> Option<(usize, Address)> {
        let api = api().ok()?;
        let (mut length, mut address) = (0u32, Address::default());
        // SAFETY: Windows writes into our buffer and address, no more than their sizes
        let ok = unsafe { (api.recv)(self.handle, buffer.as_mut_ptr().cast(), buffer.len() as u32, &mut length, &mut address) };
        (ok != 0).then_some((length as usize, address))
    }

    pub fn send(&self, packet: &[u8], address: &Address) -> bool {
        let Ok(api) = api() else { return false };
        let mut sent = 0u32;
        // SAFETY: reads our packet and address
        unsafe { (api.send)(self.handle, packet.as_ptr().cast(), packet.len() as u32, &mut sent, address) != 0 }
    }

    /// Stop receiving (a thread waiting in `recv` gets None) and sending.
    pub fn shutdown(&self) {
        if let Ok(api) = api() {
            // SAFETY: our handle
            unsafe { (api.shutdown)(self.handle, 3) }; // WINDIVERT_SHUTDOWN_BOTH
        }
    }

    /// Fill in a packet's IP and UDP/TCP checksums (packets we make ourselves).
    pub fn checksums(packet: &mut [u8], address: &mut Address) {
        if let Ok(api) = api() {
            // SAFETY: works in our packet, within its length
            unsafe { (api.checksums)(packet.as_mut_ptr().cast(), packet.len() as u32, address, 0) };
        }
    }
}

impl Drop for Divert {
    fn drop(&mut self) {
        if let Ok(api) = api() {
            // SAFETY: our handle, closed once; packets flow normally again
            unsafe { (api.close)(self.handle) };
        }
    }
}

