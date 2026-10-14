//! The DHCP server at work: the devices' DHCP requests on the hotspot's adapter come here instead
//! of to Windows (which then stays quiet), and get the helper's answers (dhcp.rs).

use super::dhcp::{self, Ip, ACK};
use super::divert::{Address, Divert, LAYER_NETWORK};
use super::{log, now, Shared};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

pub struct DhcpServer {
    divert: Arc<Divert>,
    thread: JoinHandle<()>,
}

impl DhcpServer {
    /// Answer on the hotspot's adapter; `dns`: the DNS server to give when none is set.
    pub fn start(shared: Arc<Mutex<Shared>>, gateway: Ip, dns: Ip, hotspot: u32) -> Result<DhcpServer, String> {
        let filter = format!("inbound and ifIdx == {hotspot} and ip and udp and udp.DstPort == 67");
        let divert = Arc::new(Divert::open(&filter, LAYER_NETWORK, 0)?);
        let d = divert.clone();
        let thread = std::thread::spawn(move || {
            let mut buffer = vec![0u8; 4096];
            while let Some((n, _)) = d.recv(&mut buffer) {
                let header = ((buffer[0] & 0xf) as usize) * 4;
                let Some(request) = buffer[..n].get(header + 8..).and_then(dhcp::parse) else { continue };
                let mut s = shared.lock().unwrap();
                let net = s.rules.network.clone();
                let Some((kind, ip)) = dhcp::answer(&request, &net, gateway, &mut s.leases, now()) else { continue };
                if kind == ACK && ip != [0; 4] {
                    let mac = dhcp::mac_text(&request.mac);
                    s.macs.insert(ip, mac.clone());
                    super::save("leases.json", &s.leases);
                    let name = request.hostname.clone().unwrap_or_default();
                    log::line(format!("Gave {}.{}.{}.{} to {mac} {name}", ip[0], ip[1], ip[2], ip[3]));
                }
                drop(s);
                let mut packet = dhcp::reply(&request, kind, ip, gateway, dns, &net);
                let mut address = Address::default();
                address.if_idx = hotspot;
                address.set_layer(LAYER_NETWORK);
                address.set_outbound(true);
                Divert::checksums(&mut packet, &mut address);
                d.send(&packet, &address);
            }
        });
        log::line("Giving out the hotspot's addresses (DHCP)");
        Ok(DhcpServer { divert, thread })
    }

    pub fn stop(self) {
        self.divert.shutdown();
        let _ = self.thread.join();
        log::line("Windows gives out the hotspot's addresses again");
    }
}
