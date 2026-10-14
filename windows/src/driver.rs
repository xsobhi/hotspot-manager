//! A Wi-Fi adapter's driver settings (Device Manager → the adapter → Advanced): where Windows
//! lets anyone choose a hotspot's channel and channel width, if the driver offers them (Windows'
//! hotspot itself doesn't). Read through PowerShell's Get-NetAdapterAdvancedProperty, which knows
//! the drivers' own names for them.

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
