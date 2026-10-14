//! Device makers by MAC prefix (data/oui.txt, from IEEE's list by tools/oui.py), as the Linux
//! app reads them from /usr/share/ieee-data. Phones' private addresses have no maker.

use std::collections::HashMap;
use std::sync::OnceLock;

const LIST: &str = include_str!("../data/oui.txt");

pub fn of(mac: &str) -> &'static str {
    if crate::store::random_mac(mac) {
        return "";
    }
    static MAKERS: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    let prefix: String = mac.chars().filter(char::is_ascii_hexdigit).take(6).collect::<String>().to_lowercase();
    MAKERS.get_or_init(|| LIST.lines().filter_map(|l| l.split_once(' ')).collect())
        .get(prefix.as_str()).copied().unwrap_or("")
}
