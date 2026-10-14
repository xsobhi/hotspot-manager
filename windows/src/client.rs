//! Talking to the helper service from the window (helper/pipe.rs is the other end): one JSON
//! request, one answer. Without the service, the window still does what needs no rights.

use crate::rules::{HelperStatus, Rules};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};

const PIPE: &str = r"\\.\pipe\hotspot-manager";
pub const MISSING: &str = "The Hotspot Manager service isn't installed or isn't running";

fn call<T: DeserializeOwned>(request: Value) -> Result<T, String> {
    let mut pipe = std::fs::OpenOptions::new().read(true).write(true).open(PIPE).map_err(|_| MISSING.to_string())?;
    writeln!(pipe, "{request}").map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(pipe).read_line(&mut line).map_err(|e| e.to_string())?;
    let reply: Value = serde_json::from_str(&line).map_err(|_| "The helper's answer didn't make sense".to_string())?;
    if let Some(why) = reply.get("error").and_then(Value::as_str) {
        return Err(why.to_string());
    }
    serde_json::from_value(reply.get("ok").cloned().unwrap_or(Value::Null)).map_err(|e| e.to_string())
}

pub fn status() -> Result<HelperStatus, String> {
    call(json!({ "do": "status" }))
}

pub fn rules() -> Result<Rules, String> {
    call(json!({ "do": "rules" }))
}

pub fn set_rules(rules: &Rules) -> Result<(), String> {
    call::<bool>(json!({ "do": "set_rules", "rules": rules })).map(|_| ())
}

pub fn log() -> Result<Vec<String>, String> {
    call(json!({ "do": "log" }))
}

pub fn set_gateway(gateway: &str) -> Result<(), String> {
    call::<bool>(json!({ "do": "gateway", "gateway": gateway })).map(|_| ())
}

/// Change one of a Wi-Fi adapter's driver settings (driver.rs).
pub fn set_driver(adapter: &str, keyword: &str, value: &str) -> Result<(), String> {
    call::<bool>(json!({ "do": "set_driver", "adapter": adapter, "keyword": keyword, "value": value })).map(|_| ())
}

/// Have Windows host the hotspot on this Wi-Fi adapter (None: Windows chooses).
pub fn steer(adapter: Option<windows::core::GUID>) -> Result<(), String> {
    call::<bool>(json!({ "do": "steer", "adapter": adapter.map(|a| format!("{a:?}")) })).map(|_| ())
}

/// The app's own hotspot is on (local.rs says so every few seconds) or off: the helper gives it
/// its address and shares the internet (helper/own.rs).
pub fn hosting(on: bool) -> Result<(), String> {
    call::<bool>(json!({ "do": "hosting", "on": on })).map(|_| ())
}

pub fn diagnose() -> Result<(), String> {
    call::<bool>(json!({ "do": "diagnose" })).map(|_| ())
}
