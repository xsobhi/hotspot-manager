//! The helper's door: a named pipe (\\.\pipe\hotspot-manager) that people signed in at this PC
//! may use (as polkit lets the local user run hotspotctl on Linux). A JSON line in, one out.

use super::{log, requests};
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::os::windows::io::FromRawHandle;
use windows::core::w;
use windows::Win32::Security::Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
use windows::Win32::System::Pipes::*;

pub fn serve() {
    // the system and administrators, and whoever is signed in at the PC (read and write)
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: Windows makes the descriptor from a fixed string; it lives as long as the helper
    let made = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(w!("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)"),
                                                             SDDL_REVISION_1, &mut descriptor, None)
    };
    if made.is_err() {
        log::line("Can't make the helper's pipe");
        return;
    }
    let attributes = SECURITY_ATTRIBUTES { nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                                           lpSecurityDescriptor: descriptor.0, bInheritHandle: false.into() };
    loop {
        // SAFETY: a new instance of our pipe, then waiting for someone to open it
        let pipe = unsafe {
            CreateNamedPipeW(w!(r"\\.\pipe\hotspot-manager"), PIPE_ACCESS_DUPLEX,
                             PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT, PIPE_UNLIMITED_INSTANCES, 65536, 65536, 0,
                             Some(&attributes))
        };
        if pipe.is_invalid() {
            log::line("The helper's pipe stopped working");
            return;
        }
        // SAFETY: our pipe; already connected (ERROR_PIPE_CONNECTED) counts too
        let _ = unsafe { ConnectNamedPipe(pipe, None) };
        // SAFETY: the pipe's handle, owned by the File from here on (closed with it)
        let file = unsafe { File::from_raw_handle(pipe.0) };
        std::thread::spawn(move || talk(file));
    }
}

/// Answer one window's requests until it goes.
fn talk(file: File) {
    let Ok(mut writer) = file.try_clone() else { return };
    for line in BufReader::new(file).lines() {
        let Ok(line) = line else { return };
        if line.trim().is_empty() {
            continue;
        }
        let reply = requests::answer(&line);
        if writeln!(writer, "{reply}").and_then(|()| writer.flush()).is_err() {
            return;
        }
    }
}
