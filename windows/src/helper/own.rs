//! The hotspot at an address of the user's: Windows' mobile hotspot stays at 192.168.137.1, so for
//! another one the app hosts through Wi-Fi Direct itself (local.rs, as MyPublicWiFi does) and the
//! helper shares the internet with it through Windows' classic Internet Connection Sharing, which
//! does take the address asked for (ics.rs writes it): it gives the adapter that address, the
//! devices theirs, answers their DNS and translates their traffic, in the kernel, so it's as quick
//! as the mobile hotspot (a NAT in a program, as MyPublicWiFi's, adds to every ping).
//! Without internet to share, the adapter just gets the address and the helper's DHCP serves.

use super::dhcp::Ip;
use super::{load, log, save};
use crate::driver::powershell;
use crate::net;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

const RECORD: &str = "own-hotspot.json"; // what to undo, should the helper stop before it can

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Own {
    pub index: u32,            // the Wi-Fi Direct adapter it's on
    id: String,                // its id ("{4F03…}")
    gateway: Ip,               // the address asked for
    internet: Option<u32>,     // the adapter the internet goes out of, when it started
    internet_id: String,
    shared: bool,              // through Windows' sharing (else: the address set, the helper's DHCP)
    #[serde(skip)]
    tries: (u32, Option<Instant>), // sharing it: how often, and when last
}

impl Own {
    fn start(adapter: &net::Adapter, gateway: Ip) -> Own {
        let internet = net::internet_index().and_then(|i| net::adapters().into_iter().find(|a| a.index == i));
        let mut own = Own {
            index: adapter.index,
            id: braced(&format!("{:?}", adapter.id)),
            gateway,
            internet: internet.as_ref().map(|a| a.index),
            internet_id: internet.as_ref().map(|a| braced(&format!("{:?}", a.id))).unwrap_or_default(),
            shared: false,
            tries: (0, None),
        };
        save(RECORD, &own); // (before: undone even if the helper stops halfway)
        let address = ip_text(gateway);
        if internet.is_some() {
            own.share();
        }
        if !own.shared {
            let set = format!("netsh interface ipv4 set address name={} source=static address={address} mask=255.255.255.0 | Out-Null
                               if ($LASTEXITCODE) {{ throw \"Windows didn't give the hotspot's adapter {address}\" }}", adapter.index);
            match powershell(&set) {
                Ok(_) => log::line(format!("The hotspot is at {address} on {}, without internet", adapter.friendly)),
                Err(why) => log::line(format!("Couldn't put the hotspot at {address}: {why}")),
            }
        }
        save(RECORD, &own);
        own
    }

    /// Share the internet with it (Windows' sharing sometimes refuses at first, "an event was
    /// unable to invoke any of the subscribers": tried again every ten seconds, for five minutes).
    fn share(&mut self) {
        self.tries = (self.tries.0 + 1, Some(Instant::now()));
        match share(&self.internet_id, &self.id) {
            Ok(()) => {
                self.shared = true;
                save(RECORD, &*self);
                log::line(format!("Sharing the internet with the hotspot at {}", ip_text(self.gateway)));
            }
            Err(why) if self.tries.0 == 1 => log::line(format!("Windows' sharing didn't take the hotspot (trying again): {why}")),
            Err(why) if self.tries.0 == 30 => log::line(format!("Windows' sharing still doesn't take the hotspot: {why}")),
            Err(_) => {}
        }
    }

    fn share_again(&self) -> bool {
        !self.shared && !self.internet_id.is_empty() && self.tries.0 < 30
            && self.tries.1.is_some_and(|t| t.elapsed() > Duration::from_secs(10))
    }

    /// Undo it all: sharing off, the adapter back to automatic addresses.
    fn stop(self) {
        // (shared or not: it may have stopped halfway)
        let unshare = match self.internet_id.is_empty() {
            true => String::new(),
            false => format!("{}\n", unshare_script(&[&self.internet_id, &self.id])),
        };
        let script = format!("{unshare}netsh interface ipv4 set address name={} source=dhcp | Out-Null", self.index);
        match powershell(&script) {
            Ok(_) => {
                let _ = std::fs::remove_file(log::folder().join(RECORD));
                log::line("The hotspot's own sharing is undone");
            }
            // (the record stays: tried again when the helper next starts)
            Err(why) => log::line(format!("Couldn't undo the hotspot's sharing: {why}")),
        }
    }

    /// Does the helper give out the addresses? (Windows' sharing does, when it's on.)
    pub fn needs_dhcp(&self) -> bool {
        !self.shared
    }
}

/// Windows' classic sharing: the internet connection shared (public), with the hotspot's adapter
/// (private). Only one connection can be shared: any other stops being.
fn share(public: &str, private: &str) -> Result<(), String> {
    powershell(&format!("$ErrorActionPreference = 'Stop'
        $m = New-Object -ComObject HNetCfg.HNetShare
        $public = $null; $private = $null
        foreach ($c in @($m.EnumEveryConnection)) {{
            $guid = $m.NetConnectionProps.Invoke($c).Guid
            $s = $m.INetSharingConfigurationForINetConnection.Invoke($c)
            if ($guid -eq '{public}') {{ $public = $s }}
            elseif ($guid -eq '{private}') {{ $private = $s }}
            elseif ($s.SharingEnabled) {{ $s.DisableSharing() }}
        }}
        if (-not $public) {{ throw \"it doesn't list the internet connection\" }}
        if (-not $private) {{ throw \"it doesn't list the hotspot's adapter\" }}
        if ($public.SharingEnabled -and $public.SharingConnectionType -ne 0) {{ $public.DisableSharing() }}
        if ($private.SharingEnabled -and $private.SharingConnectionType -ne 1) {{ $private.DisableSharing() }}
        if (-not $public.SharingEnabled) {{ $public.EnableSharing(0) }}
        if (-not $private.SharingEnabled) {{ $private.EnableSharing(1) }}")).map(|_| ())
}

fn unshare_script(ids: &[&str]) -> String {
    let ids = ids.iter().map(|id| format!("'{id}'")).collect::<Vec<_>>().join(",");
    format!("$m = New-Object -ComObject HNetCfg.HNetShare
        foreach ($c in @($m.EnumEveryConnection)) {{
            if (@({ids}) -contains $m.NetConnectionProps.Invoke($c).Guid) {{
                $s = $m.INetSharingConfigurationForINetConnection.Invoke($c)
                if ($s.SharingEnabled) {{ $s.DisableSharing() }}
            }}
        }}")
}

/// Every two seconds: while the app hosts its own hotspot (`hosting`), it's shared, on the
/// Wi-Fi Direct adapter it's on, at the address asked for; then all of it's undone.
pub fn follow(hosting: bool, own: &mut Option<Own>) {
    let wanted = net::gateway();
    let all: Vec<net::Adapter> = if hosting {
        net::adapters().into_iter().filter(|a| a.up && a.name.contains("Wi-Fi Direct")).collect()
    } else {
        vec![]
    };
    // the one it's on already, else the one that came up (Windows' mobile hotspot is off meanwhile)
    let current = own.as_ref().map(|o| o.index);
    let adapter = all.iter().find(|a| Some(a.index) == current).or_else(|| all.first());
    if let Some(o) = own.as_mut() {
        let same = adapter.is_some_and(|a| a.index == o.index) && o.gateway == wanted && o.internet == net::internet_index();
        if same {
            if o.share_again() {
                o.share();
            }
            return;
        }
        own.take().unwrap().stop();
    }
    if let Some(a) = adapter {
        *own = Some(Own::start(a, wanted));
    }
}

/// When the helper starts: undo what it left on when it stopped (if it stopped while hosting).
pub fn leftovers() {
    let left: Own = load(RECORD);
    if left.index != 0 {
        left.stop();
    }
}

fn braced(guid: &str) -> String {
    format!("{{{}}}", guid.trim_matches(|c| c == '{' || c == '}'))
}

fn ip_text(ip: Ip) -> String {
    format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3])
}
