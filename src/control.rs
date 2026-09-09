use crate::{
    config::{self, Config},
    input::{self, MouseInfo},
};
use serde::{Deserialize, Serialize};
use std::{
    io::{self, BufRead, Read, Write},
    os::unix::net::UnixStream,
    time::Duration,
};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Status,
    Enable,
    Disable,
    Toggle,
    SetAcceleration { value: f64 },
    SetCoast { value: bool },
    Select { id: String, value: bool },
    Stop,
}
pub fn parse_request(bytes: &[u8]) -> Result<Request, String> {
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let object = value.as_object().ok_or("Request must be an object")?;
    let command = object
        .get("command")
        .and_then(|v| v.as_str())
        .ok_or("Missing command")?;
    let allowed: &[&str] = match command {
        "set_acceleration" | "set_coast" => &["command", "value"],
        "select" => &["command", "id", "value"],
        _ => &["command"],
    };
    if object.keys().any(|k| !allowed.contains(&k.as_str())) {
        return Err("Unknown request field".into());
    }
    serde_json::from_value(value).map_err(|e| e.to_string())
}
#[derive(Serialize, Deserialize)]
pub struct Status {
    pub running: bool,
    pub enabled: bool,
    pub acceleration: f64,
    pub coast: bool,
    pub devices: Vec<MouseInfo>,
    #[serde(default)]
    pub active_devices: Vec<String>,
    #[serde(default)]
    pub preparing_devices: Vec<String>,
    pub errors: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
impl Status {
    pub fn new(config: &Config, mut devices: Vec<MouseInfo>, errors: Vec<String>) -> Self {
        for device in &mut devices {
            device.selected = config.selected_devices.contains(&device.id);
        }
        Self {
            running: true,
            enabled: config.enabled,
            acceleration: config.acceleration,
            coast: config.coast,
            devices,
            active_devices: vec![],
            preparing_devices: vec![],
            error: None,
            errors,
        }
    }
}
pub fn apply(config: &Config, request: &Request) -> Result<Config, String> {
    let mut next = config.clone();
    match request {
        Request::Enable => next.enabled = true,
        Request::Disable => next.enabled = false,
        Request::Toggle => next.enabled = !next.enabled,
        Request::SetAcceleration { value } => next.acceleration = *value,
        Request::SetCoast { value } => next.coast = *value,
        Request::Select { id, value } => {
            next.selected_devices.retain(|item| item != id);
            if *value {
                next.selected_devices.push(id.clone());
            }
        }
        Request::Status | Request::Stop => {}
    }
    next.validate()?;
    Ok(next)
}
pub fn send(request: &Request) -> io::Result<Status> {
    let path = config::runtime_dir()?.join("control.sock");
    let mut stream = UnixStream::connect(path).map_err(|e| io::Error::new(e.kind(), format!("Velvet Scroll is not running. Start it with 'velvet-scroll daemon' or the desktop app. ({e})")))?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    serde_json::to_writer(&mut stream, request)?;
    stream.write_all(b"\n")?;
    let mut line = String::new();
    io::BufReader::new(stream)
        .take(65537)
        .read_line(&mut line)?;
    if line.len() > 65536 {
        return Err(io::Error::other("Control response is too large"));
    }
    let status: Status = serde_json::from_str(&line)?;
    Ok(status)
}
pub fn offline_devices() -> io::Result<Status> {
    let config = config::load()?;
    let mut status = Status::new(&config, input::discover()?, vec![]);
    status.running = false;
    Ok(status)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changes_validate_before_mutation() {
        let c = Config::default();
        assert!(apply(&c, &Request::SetAcceleration { value: 99.0 }).is_err());
        assert_eq!(c.acceleration, 3.0);
        let c = apply(
            &c,
            &Request::Select {
                id: "mouse-a".into(),
                value: true,
            },
        )
        .unwrap();
        let c = apply(
            &c,
            &Request::Select {
                id: "mouse-a".into(),
                value: true,
            },
        )
        .unwrap();
        assert_eq!(c.selected_devices.len(), 1);
        assert!(apply(
            &c,
            &Request::Select {
                id: "mouse-a".into(),
                value: false
            }
        )
        .unwrap()
        .selected_devices
        .is_empty());
    }
    #[test]
    fn malformed_commands_are_rejected() {
        for json in [
            r#"{"command":"shell"}"#,
            r#"{"command":"toggle","extra":1}"#,
            r#"{"command":"set_coast","value":"yes"}"#,
        ] {
            assert!(parse_request(json.as_bytes()).is_err());
        }
    }
}
