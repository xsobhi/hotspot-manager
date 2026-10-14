//! A Wi-Fi adapter's driver settings (Device Manager → the adapter → Advanced): where Windows
//! lets anyone choose a hotspot's channel and channel width, if the driver offers them (Windows'
//! hotspot itself doesn't). Read through PowerShell's Get-NetAdapterAdvancedProperty, which knows
//! the drivers' own names for them.

use crate::hotspot::Band;
use serde::Deserialize;
use std::os::windows::process::CommandExt;

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Property {
    #[serde(rename = "DisplayName")]
    pub name: String,
    #[serde(rename = "DisplayValue")]
    pub value: String,
    #[serde(rename = "RegistryKeyword")]
    pub keyword: String,
    #[serde(rename = "ValidDisplayValues")]
    choices_text: String, // one a line (Windows PowerShell's JSON mangles lists)
    #[serde(rename = "ValidRegistryValues")]
    values_text: String,
}

impl Property {
    /// Its choices, as shown.
    pub fn choices(&self) -> Vec<String> {
        self.choices_text.lines().map(String::from).collect()
    }

    /// Its choices, as the driver stores them.
    pub fn values(&self) -> Vec<String> {
        self.values_text.lines().map(String::from).collect()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Channel,
    Width,
    Mode,
}

/// What a driver setting is about, from its name.
pub fn kind(p: &Property) -> Option<Kind> {
    let name = p.name.to_lowercase();
    if name.contains("width") || name.contains("bandwidth") || name.contains("ht mode") {
        Some(Kind::Width)
    } else if name.contains("channel") && !name.contains("ad hoc") && !name.contains("adhoc") {
        Some(Kind::Channel)
    } else if name.contains("wireless mode") || name.contains("802.11 mode") || name == "mode" {
        Some(Kind::Mode)
    } else {
        None
    }
}

/// Which band a Wireless Mode choice keeps the adapter to, if one ("IEEE 802.11a/n/ac": 5 GHz,
/// "IEEE 802.11b/g/n": 2.4 GHz, "Auto" or "802.11a/b/g": none), and how many standards it has.
fn reach(choice: &str) -> (Option<Band>, usize) {
    let lower = choice.to_lowercase();
    if lower.contains("only") {
        if lower.contains("2.4") {
            return (Some(Band::TwoPointFour), 1);
        }
        if lower.contains('5') {
            return (Some(Band::Five), 1);
        }
    }
    let Some(at) = lower.find("802.11") else { return (None, 0) };
    let standards: Vec<&str> = lower[at + 6..].split(|c: char| !c.is_ascii_alphanumeric()).filter(|s| !s.is_empty()).collect();
    let five = standards.iter().any(|s| *s == "a" || *s == "ac");
    let two = standards.iter().any(|s| *s == "b" || *s == "g");
    let band = match (five, two) {
        (true, false) => Some(Band::Five),
        (false, true) => Some(Band::TwoPointFour),
        _ => None,
    };
    (band, standards.len())
}

/// Does this Wireless Mode choice keep the adapter to one band?
pub fn single_band(choice: &str) -> bool {
    reach(choice).0.is_some()
}

/// The Wireless Mode choice for the hotspot's band: the most capable one kept to that band, or for
/// any band "Auto" (else the widest). (Its value as the driver stores it, and as shown.)
pub fn mode_for(p: &Property, band: Band) -> Option<(String, String)> {
    let choices: Vec<(String, String)> = p.choices().into_iter().zip(p.values()).collect();
    let pick = match band {
        Band::Any => choices.iter().find(|(c, _)| c.to_lowercase().contains("auto"))
            .or_else(|| choices.iter().filter(|(c, _)| reach(c).0.is_none()).max_by_key(|(c, _)| reach(c).1)),
        wanted => choices.iter().filter(|(c, _)| reach(c).0 == Some(wanted)).max_by_key(|(c, _)| reach(c).1),
    };
    pick.map(|(shown, value)| (value.clone(), shown.clone()))
}

/// PowerShell's text for a string (single quotes, doubled inside).
pub fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// Run a PowerShell command (no window); its output, or why it failed.
pub fn powershell(script: &str) -> Result<String, String> {
    let output = std::process::Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(0x0800_0000) // no console window
        .output().map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).lines().find(|l| !l.trim().is_empty()).unwrap_or("PowerShell failed").to_string())
    }
}

/// The adapter's driver settings that have a list of choices (by its description).
pub fn properties(description: &str) -> Vec<Property> {
    let script = format!(
        "ConvertTo-Json -Depth 3 -InputObject @(Get-NetAdapterAdvancedProperty -InterfaceDescription {} -ErrorAction \
         SilentlyContinue | Select-Object DisplayName, DisplayValue, RegistryKeyword, \
         @{{n='ValidDisplayValues';e={{$_.ValidDisplayValues -join [char]10}}}},          @{{n='ValidRegistryValues';e={{$_.ValidRegistryValues -join [char]10}}}})",
        quoted(description));
    let output = std::process::Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(0x0800_0000) // no console window
        .output();
    let Ok(output) = output else { return vec![] };
    let list: Vec<Property> = serde_json::from_slice(&output.stdout).unwrap_or_default();
    list.into_iter().filter(|p| !p.choices_text.is_empty() && p.choices().len() == p.values().len()).collect()
}

/// The driver's settings for the channel, its width and the Wi-Fi mode, if it has them.
pub fn hotspot_settings(description: &str) -> Vec<(Kind, Property)> {
    let mut found: Vec<(Kind, Property)> = properties(description).into_iter().filter_map(|p| kind(&p).map(|k| (k, p))).collect();
    found.sort_by_key(|(k, _)| *k as u8);
    found
}

/// Change a driver setting (needs administrator rights: the helper does it). Windows restarts
/// the adapter for it.
pub fn set(description: &str, keyword: &str, value: &str) -> Result<(), String> {
    let property = properties(description).into_iter().find(|p| p.keyword == keyword)
        .ok_or("The adapter's driver has no such setting")?;
    if !property.values().iter().any(|v| v == value) {
        return Err(format!("{} can't be set to that", property.name));
    }
    let script = format!("Set-NetAdapterAdvancedProperty -InterfaceDescription {} -RegistryKeyword {} -RegistryValue {} -ErrorAction Stop",
                         quoted(description), quoted(keyword), quoted(value));
    let output = std::process::Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(0x0800_0000)
        .output().map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!("Windows didn't change {}: {}", property.name, String::from_utf8_lossy(&output.stderr).lines().next().unwrap_or("")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(choices: &[&str], values: &[&str], now: &str) -> Property {
        Property { name: "Wireless Mode".into(), value: now.into(), keyword: "WirelessMode".into(),
                   choices_text: choices.join("\n"), values_text: values.join("\n") }
    }

    #[test]
    fn band_modes() {
        // the TP-Link Archer T4U's (Realtek, Microsoft's driver)
        let tp_link = mode(&["Auto", "IEEE 802.11a", "IEEE 802.11b", "IEEE 802.11b/g", "IEEE 802.11b/g/n", "IEEE 802.11a/n",
                             "IEEE 802.11a/n/ac", "IEEE 802.11ac"], &["8", "1", "2", "4", "16", "32", "64", "256"], "Auto");
        assert_eq!(mode_for(&tp_link, Band::Five), Some(("64".into(), "IEEE 802.11a/n/ac".into())));
        assert_eq!(mode_for(&tp_link, Band::TwoPointFour), Some(("16".into(), "IEEE 802.11b/g/n".into())));
        assert_eq!(mode_for(&tp_link, Band::Any), Some(("8".into(), "Auto".into())));
        // Intel's style
        let intel = mode(&["1. 802.11a", "2. 802.11b", "3. 802.11g", "4. 802.11a/b", "6. 802.11a/b/g"], &["1", "2", "3", "4", "6"], "6. 802.11a/b/g");
        assert_eq!(mode_for(&intel, Band::Five), Some(("1".into(), "1. 802.11a".into())));
        assert_eq!(mode_for(&intel, Band::Any), Some(("6".into(), "6. 802.11a/b/g".into())));
        assert!(single_band("IEEE 802.11a/n/ac") && single_band("5GHz only") && !single_band("Auto") && !single_band("6. 802.11a/b/g"));
    }
}
