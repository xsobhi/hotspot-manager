//! What Windows knows about the network adapters: their names ("TP-Link Wireless MU-MIMO USB
//! Adapter"), which one is the hotspot's and how much it has sent and received (its live speed),
//! and the devices next to it (their MAC addresses, from Windows' neighbour table).

use windows::core::{w, GUID};
use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, NO_ERROR};
use windows::Win32::NetworkManagement::IpHelper::*;
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::Networking::WinSock::*;
use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};

/// The hotspot's address on the PC, which Internet Connection Sharing gives its adapter.
pub fn gateway() -> [u8; 4] {
    let mut buffer = [0u16; 32];
    let mut size = (buffer.len() * 2) as u32;
    // SAFETY: reads a short string into our buffer, no more than its size
    let read = unsafe {
        RegGetValueW(HKEY_LOCAL_MACHINE, w!(r"SYSTEM\CurrentControlSet\Services\SharedAccess\Parameters"),
                     w!("ScopeAddress"), RRF_RT_REG_SZ, None, Some(buffer.as_mut_ptr().cast()), Some(&mut size))
    };
    let text = String::from_utf16_lossy(&buffer[..buffer.iter().position(|&c| c == 0).unwrap_or(0)]);
    let parts: Option<Vec<u8>> = text.split('.').map(|p| p.trim().parse().ok()).collect();
    match (read.is_ok(), parts.and_then(|p| <[u8; 4]>::try_from(p).ok())) {
        (true, Some(ip)) => ip,
        _ => [192, 168, 137, 1],
    }
}

/// The hotspot's adapter, while it's on: the one with the hotspot's address.
pub fn hotspot_adapter() -> Option<Adapter> {
    let gateway = gateway();
    adapters().into_iter().find(|a| a.up && a.ipv4.contains(&gateway))
}

/// The devices Windows has seen on an adapter lately: (address, MAC "aa:bb:…").
pub fn neighbors(index: u32) -> Vec<([u8; 4], String)> {
    let mut table: *mut MIB_IPNET_TABLE2 = std::ptr::null_mut();
    // SAFETY: Windows allocates the table; we read its rows and free it
    unsafe {
        if GetIpNetTable2(AF_INET, &mut table) != NO_ERROR || table.is_null() {
            return vec![];
        }
        let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize);
        let live = [NlnsReachable, NlnsStale, NlnsDelay, NlnsProbe, NlnsPermanent];
        let found = rows.iter().filter(|r| r.InterfaceIndex == index && live.contains(&r.State) && r.PhysicalAddressLength == 6)
            .map(|r| {
                let b = r.Address.Ipv4.sin_addr.S_un.S_un_b;
                let mac = r.PhysicalAddress[..6].iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(":");
                ([b.s_b1, b.s_b2, b.s_b3, b.s_b4], mac)
            })
            .filter(|(ip, mac)| ip[3] != 255 && mac != "ff:ff:ff:ff:ff:ff" && !mac.starts_with("01:00:5e"))
            .collect();
        FreeMibTable(table.cast());
        found
    }
}

pub struct Adapter {
    pub id: GUID,
    pub index: u32,
    pub name: String,     // its description: the hardware
    pub friendly: String, // "Wi-Fi 2"
    pub up: bool,
    pub wifi: bool, // a Wi-Fi adapter itself (not Windows' virtual ones on it)
    pub ipv4: Vec<[u8; 4]>,
}

/// Every network adapter, with its addresses.
pub fn adapters() -> Vec<Adapter> {
    let flags = GAA_FLAG_SKIP_DNS_SERVER | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_ANYCAST;
    let mut size = 16 * 1024u32;
    let mut buffer: Vec<u64> = vec![];
    // SAFETY: Windows fills our buffer (8-byte aligned), asking for a bigger one when needed
    let result = loop {
        buffer.resize(size as usize / 8 + 1, 0);
        let first = buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;
        match unsafe { GetAdaptersAddresses(AF_UNSPEC.0 as u32, flags, None, Some(first), &mut size) } {
            e if e == ERROR_BUFFER_OVERFLOW.0 => continue,
            e => break e,
        }
    };
    if result != NO_ERROR.0 {
        return vec![];
    }
    let mut list = vec![];
    let mut next = buffer.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
    // SAFETY: walking the list Windows wrote into `buffer`, which outlives this loop
    unsafe {
        while let Some(a) = next.as_ref() {
            let id = a.AdapterName.to_string().ok()
                .and_then(|n| GUID::try_from(n.trim_matches(|c| c == '{' || c == '}')).ok());
            let mut ipv4 = vec![];
            let mut address = a.FirstUnicastAddress;
            while let Some(u) = address.as_ref() {
                let sockaddr = u.Address.lpSockaddr;
                if !sockaddr.is_null() && (*sockaddr).sa_family == AF_INET {
                    let b = (*(sockaddr as *const SOCKADDR_IN)).sin_addr.S_un.S_un_b;
                    ipv4.push([b.s_b1, b.s_b2, b.s_b3, b.s_b4]);
                }
                address = u.Next;
            }
            if let Some(id) = id {
                list.push(Adapter {
                    id,
                    index: a.Anonymous1.Anonymous.IfIndex,
                    name: a.Description.to_string().unwrap_or_default(),
                    friendly: a.FriendlyName.to_string().unwrap_or_default(),
                    up: a.OperStatus == IfOperStatusUp,
                    wifi: a.IfType == IF_TYPE_IEEE80211 && !virtual_adapter(&a.Description.to_string().unwrap_or_default()),
                    ipv4,
                });
            }
            next = a.Next;
        }
    }
    list
}

fn virtual_adapter(description: &str) -> bool {
    description.contains("Virtual") || description.contains("Wi-Fi Direct")
}

/// Bytes (sent to the devices, received from them) through the hotspot so far, while it's on.
pub fn hotspot_traffic() -> Option<(u64, u64)> {
    let adapter = hotspot_adapter()?;
    let mut row = MIB_IF_ROW2 { InterfaceIndex: adapter.index, ..Default::default() };
    // SAFETY: Windows fills the row of the interface we name
    (unsafe { GetIfEntry2(&mut row) } == NO_ERROR).then_some((row.OutOctets, row.InOctets))
}

