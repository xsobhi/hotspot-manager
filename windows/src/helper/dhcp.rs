//! The helper's own DHCP server, as dnsmasq is for the Linux app: when the hotspot's addresses
//! should be other than what Windows hands out (another range, lease time, DNS servers, fixed
//! addresses), the devices' DHCP requests come to the helper instead of Windows' (WinDivert) and
//! it answers them. Windows still routes (NATs) their traffic: their addresses are in its subnet.

use crate::rules::Network;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const DISCOVER: u8 = 1;
pub const OFFER: u8 = 2;
pub const REQUEST: u8 = 3;
pub const DECLINE: u8 = 4;
pub const ACK: u8 = 5;
pub const NAK: u8 = 6;
pub const RELEASE: u8 = 7;
pub const INFORM: u8 = 8;
const COOKIE: [u8; 4] = [99, 130, 83, 99];

pub type Ip = [u8; 4];

#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub kind: u8,
    pub xid: [u8; 4],
    pub flags: [u8; 2],
    pub ciaddr: Ip,
    pub mac: [u8; 6],
    pub requested: Option<Ip>, // option 50
    pub server: Option<Ip>,    // option 54
    pub hostname: Option<String>,
}

pub fn mac_text(mac: &[u8; 6]) -> String {
    mac.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":")
}

pub fn parse_ip(text: &str) -> Option<Ip> {
    let parts: Vec<u8> = text.trim().split('.').map(|p| p.parse().ok()).collect::<Option<_>>()?;
    parts.try_into().ok()
}

/// A DHCP request, from a UDP packet's payload (BOOTP with DHCP options).
pub fn parse(payload: &[u8]) -> Option<Request> {
    if payload.len() < 240 || payload[0] != 1 || payload[1] != 1 || payload[2] != 6 || payload[236..240] != COOKIE {
        return None;
    }
    let ip = |at: usize| -> Ip { payload[at..at + 4].try_into().unwrap() };
    let mut request = Request {
        kind: 0, xid: payload[4..8].try_into().unwrap(), flags: payload[10..12].try_into().unwrap(), ciaddr: ip(12),
        mac: payload[28..34].try_into().unwrap(), requested: None, server: None, hostname: None,
    };
    let mut at = 240;
    while at < payload.len() {
        let code = payload[at];
        if code == 255 {
            break;
        }
        if code == 0 {
            at += 1;
            continue;
        }
        let len = *payload.get(at + 1)? as usize;
        let value = payload.get(at + 2..at + 2 + len)?;
        match (code, len) {
            (53, 1) => request.kind = value[0],
            (50, 4) => request.requested = Some(value.try_into().unwrap()),
            (54, 4) => request.server = Some(value.try_into().unwrap()),
            (12, _) => request.hostname = Some(String::from_utf8_lossy(value).into_owned()),
            _ => {}
        }
        at += 2 + len;
    }
    (request.kind != 0).then_some(request)
}

/// The addresses given out: MAC → (address, until when).
#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Leases {
    pub given: BTreeMap<String, (Ip, u64)>,
}

impl Leases {
    /// The address for `mac`: its fixed one, the one it has, or the first free one in the range.
    pub fn choose(&self, mac: &str, net: &Network, gateway: Ip, now: u64) -> Option<Ip> {
        let subnet = [gateway[0], gateway[1], gateway[2]];
        if let Some(fixed) = net.fixed.get(mac).and_then(|ip| parse_ip(ip)) {
            return Some(fixed);
        }
        let fixed: Vec<Ip> = net.fixed.values().filter_map(|ip| parse_ip(ip)).collect();
        let usable = |ip: Ip| ip[..3] == subnet && ip != gateway && (net.dhcp_from..=net.dhcp_to).contains(&ip[3])
            && !fixed.contains(&ip);
        if let Some((ip, _)) = self.given.get(mac).filter(|(ip, _)| usable(*ip)) {
            return Some(*ip);
        }
        let taken = |ip: Ip| self.given.iter().any(|(m, (i, until))| m != mac && *i == ip && *until > now);
        (net.dhcp_from.max(1)..=net.dhcp_to.min(254)).map(|n| [subnet[0], subnet[1], subnet[2], n])
            .find(|ip| usable(*ip) && !taken(*ip))
    }
}

/// What to answer (kind, address), and the leases changed: None when there's nothing to say.
pub fn answer(req: &Request, net: &Network, gateway: Ip, leases: &mut Leases, now: u64) -> Option<(u8, Ip)> {
    let mac = mac_text(&req.mac);
    match req.kind {
        DISCOVER => leases.choose(&mac, net, gateway, now).map(|ip| (OFFER, ip)),
        REQUEST => {
            if req.server.is_some_and(|s| s != gateway) {
                return None; // it chose another server's offer
            }
            let wanted = req.requested.or((req.ciaddr != [0; 4]).then_some(req.ciaddr))?;
            match leases.choose(&mac, net, gateway, now) {
                Some(ip) if ip == wanted => {
                    leases.given.insert(mac, (ip, now + net.lease_hours.max(1) as u64 * 3600));
                    Some((ACK, ip))
                }
                _ => Some((NAK, [0; 4])),
            }
        }
        RELEASE | DECLINE => {
            leases.given.remove(&mac);
            None
        }
        INFORM => Some((ACK, [0; 4])),
        _ => None,
    }
}

/// The reply as a whole IPv4 packet from the hotspot (checksums left for WinDivert to fill in).
pub fn reply(req: &Request, kind: u8, yiaddr: Ip, gateway: Ip, net: &Network) -> Vec<u8> {
    let mut bootp = vec![0u8; 240];
    bootp[0] = 2; // a reply
    bootp[1] = 1;
    bootp[2] = 6;
    bootp[4..8].copy_from_slice(&req.xid);
    bootp[10..12].copy_from_slice(&req.flags);
    if kind == ACK && req.kind == INFORM {
        bootp[12..16].copy_from_slice(&req.ciaddr);
    }
    bootp[16..20].copy_from_slice(&yiaddr);
    bootp[28..34].copy_from_slice(&req.mac);
    bootp[236..240].copy_from_slice(&COOKIE);
    let mut option = |code: u8, value: &[u8]| {
        bootp.push(code);
        bootp.push(value.len() as u8);
        bootp.extend_from_slice(value);
    };
    option(53, &[kind]);
    option(54, &gateway);
    if kind != NAK {
        let lease = net.lease_hours.max(1) * 3600;
        if req.kind != INFORM {
            option(51, &lease.to_be_bytes());
            option(58, &(lease / 2).to_be_bytes());
            option(59, &(lease / 8 * 7).to_be_bytes());
        }
        option(1, &[255, 255, 255, 0]);
        option(3, &gateway);
        let dns: Vec<u8> = net.dns.iter().filter_map(|d| parse_ip(d)).take(3).flatten().collect();
        option(6, if dns.is_empty() { &gateway } else { &dns });
    }
    bootp.push(255);
    bootp.resize(bootp.len().max(300), 0); // the smallest BOOTP message
    // to the device's address when it has one, else to everyone (it can't take one yet)
    let to = if req.ciaddr != [0; 4] && kind == ACK { req.ciaddr } else { [255; 4] };
    let total = 20 + 8 + bootp.len();
    let mut packet = Vec::with_capacity(total);
    packet.extend_from_slice(&[0x45, 0, (total >> 8) as u8, total as u8, 0, 0, 0, 0, 64, 17, 0, 0]);
    packet.extend_from_slice(&gateway);
    packet.extend_from_slice(&to);
    let udp = 8 + bootp.len();
    packet.extend_from_slice(&[0, 67, 0, 68, (udp >> 8) as u8, udp as u8, 0, 0]);
    packet.extend_from_slice(&bootp);
    packet
}

#[cfg(test)]
#[path = "dhcp_tests.rs"]
mod tests;
