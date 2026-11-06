//! Engine of the remote module. With remote access on, this device answers
//! on one port: NipaPlay's API (`share`) for its clients, and the WebUI for a
//! browser. The engine says what it wants of the server and shows how it
//! stands; requests are answered on the server's own threads, each with a
//! database connection of the module's `Share`.

#[cfg(not(target_arch = "wasm32"))]
mod discovery;
#[cfg(not(target_arch = "wasm32"))]
pub mod share;

#[cfg(not(target_arch = "wasm32"))]
use bern::web_ui::Options;
use bern::web_ui;
#[cfg(not(target_arch = "wasm32"))]
use bern::web_ui::Status;
use bern::{CommandError, resources::Resources};
#[cfg(not(target_arch = "wasm32"))]
use discovery::Discovery;
use erika_remote_protocol::remote::{self, Address, Device, State};
use erika_store::Db;
use serde_json::Value;
#[cfg(not(target_arch = "wasm32"))]
use share::Share;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const ENABLED: &str = "remote.enabled";
const PORT: &str = "remote.port";
const RECEIVER: &str = "remote.receiver";
const TRUSTED: &str = "remote.trusted";
const DEFAULT_PORT: i64 = 1180;
/// How long a refused device stays refused.
const DENIED_FOR: Duration = Duration::from_secs(8);

pub struct RemoteEngine {
    state: State,
    db: Db,
    started: bool,
    #[cfg(not(target_arch = "wasm32"))]
    discovery: Option<Discovery>,
    receiver: Arc<AtomicBool>,
    /// Allowed until the app quits.
    approved: HashSet<String>,
    denied: HashMap<String, Instant>,
}

impl Default for RemoteEngine {
    fn default() -> Self {
        Self {
            state: State {
                available: false,
                enabled: false,
                running: false,
                port: DEFAULT_PORT,
                addresses: Vec::new(),
                error: String::new(),
                receiver: true,
                asking: None,
                trusted: Vec::new(),
                has_trusted: false,
            },
            db: Db::open(),
            started: false,
            #[cfg(not(target_arch = "wasm32"))]
            discovery: None,
            receiver: Arc::new(AtomicBool::new(true)),
            approved: HashSet::new(),
            denied: HashMap::new(),
        }
    }
}

impl Drop for RemoteEngine {
    fn drop(&mut self) {
        web_ui::set(None);
    }
}

fn device_from(value: &Value) -> Device {
    let text = |name: &str| value[name].as_str().unwrap_or_default().to_string();
    Device {
        key: text("key"),
        name: text("name"),
        platform: text("platform"),
        address: text("address"),
    }
}

fn device_json(device: &Device) -> Value {
    serde_json::json!({
        "key": device.key,
        "name": device.name,
        "platform": device.platform,
        "address": device.address,
    })
}

/// The addresses other devices reach this one by.
#[cfg(not(target_arch = "wasm32"))]
fn addresses(port: i64) -> Vec<Address> {
    let mut found: Vec<String> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .iter()
        .filter(|interface| !interface.is_loopback() && interface.ip().is_ipv4())
        .map(|interface| format!("http://{}:{port}/", interface.ip()))
        .collect();
    found.sort();
    found.dedup();
    found.into_iter().map(|url| Address { url }).collect()
}

impl RemoteEngine {
    fn load(&mut self) {
        // An engine host has no window: serving is what it is for.
        let host = Resources::host().is_served();
        self.state.available = host || web_ui::page().is_some();
        self.state.enabled = match self.db.setting(ENABLED).as_deref() {
            Some(value) => value == "true",
            None => host,
        };
        self.state.port = self
            .db
            .setting(PORT)
            .and_then(|port| port.parse().ok())
            .unwrap_or(DEFAULT_PORT);
        self.state.receiver = self.db.setting(RECEIVER).as_deref() != Some("false");
        self.receiver.store(self.state.receiver, Ordering::Relaxed);
        self.state.trusted = self
            .db
            .setting(TRUSTED)
            .and_then(|text| serde_json::from_str::<Vec<Value>>(&text).ok())
            .unwrap_or_default()
            .iter()
            .map(device_from)
            .collect();
        self.state.has_trusted = !self.state.trusted.is_empty();
    }

    fn save_trusted(&mut self) {
        self.state.has_trusted = !self.state.trusted.is_empty();
        let list: Vec<Value> = self.state.trusted.iter().map(device_json).collect();
        self.db
            .set_setting(TRUSTED, Some(&Value::Array(list).to_string()));
    }

    /// An engine in a browser has no server to ask for.
    #[cfg(target_arch = "wasm32")]
    fn apply(&mut self) {}

    #[cfg(target_arch = "wasm32")]
    fn observe(&mut self) {}

    /// Ask for the server as the settings say, and look again shortly: the
    /// runtime may start it only once the engine is up.
    #[cfg(not(target_arch = "wasm32"))]
    fn apply(&mut self) {
        if self.state.enabled && self.state.available {
            web_ui::set(Some(Options {
                port: self.state.port as u16,
                routes: Some(Arc::new(Share::new(Db::open(), Arc::clone(&self.receiver)))),
            }));
        } else {
            web_ui::set(None);
        }
        self.observe();
        if let Some(waker) = bern::wake::waker() {
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(400));
                waker.wake();
            });
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn observe(&mut self) {
        match web_ui::status() {
            Status::Running { port } => {
                self.state.running = true;
                self.state.error.clear();
                self.state.addresses = addresses(i64::from(port));
                if self.discovery.is_none() {
                    // Without it NipaPlay still connects by address.
                    self.discovery = Discovery::start(port).ok();
                }
            }
            Status::Stopped => {
                self.state.running = false;
                self.state.addresses.clear();
                self.discovery = None;
            }
            Status::Failed(message) => {
                self.state.running = false;
                self.state.error = message;
                self.state.addresses.clear();
                self.discovery = None;
            }
        }
    }
}

impl remote::Commands for RemoteEngine {
    fn set_enabled(&mut self, enabled: bool) -> Result<(), CommandError> {
        self.state.enabled = enabled;
        self.db.set_setting(ENABLED, Some(&enabled.to_string()));
        self.apply();
        Ok(())
    }

    fn set_port(&mut self, port: String) -> Result<(), CommandError> {
        let port: i64 = port.trim().parse().unwrap_or(0);
        if !(1..=65535).contains(&port) {
            return Err(CommandError::new("invalid_port", "A port is 1 to 65535"));
        }
        self.state.port = port;
        self.db.set_setting(PORT, Some(&port.to_string()));
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.discovery = None;
        }
        self.apply();
        Ok(())
    }

    fn set_receiver(&mut self, enabled: bool) -> Result<(), CommandError> {
        self.state.receiver = enabled;
        self.receiver.store(enabled, Ordering::Relaxed);
        self.db.set_setting(RECEIVER, Some(&enabled.to_string()));
        Ok(())
    }

    fn copy_address(&mut self, url: String) -> Result<(), CommandError> {
        bern::platform::copy_text(&url)
    }

    fn answer(&mut self, allow: bool, trust: bool) -> Result<(), CommandError> {
        let Some(device) = self.state.asking.take() else {
            return Ok(());
        };
        if !allow {
            self.denied.insert(device.key, Instant::now());
        } else if trust {
            self.approved.insert(device.key.clone());
            self.state.trusted.retain(|known| known.key != device.key);
            self.state.trusted.push(device);
            self.save_trusted();
        } else {
            self.approved.insert(device.key);
        }
        Ok(())
    }

    fn forget_device(&mut self, key: String) -> Result<(), CommandError> {
        self.approved.remove(&key);
        self.state.trusted.retain(|device| device.key != key);
        self.save_trusted();
        Ok(())
    }

    fn authorize(&mut self, client: String, ask: bool) -> Result<String, CommandError> {
        let device = device_from(&serde_json::from_str(&client).unwrap_or(Value::Null));
        let known = self.approved.contains(&device.key)
            || self.state.trusted.iter().any(|other| other.key == device.key);
        let refused = self
            .denied
            .get(&device.key)
            .is_some_and(|at| at.elapsed() < DENIED_FOR);
        let answer = if known {
            "authorized"
        } else if !ask || !self.state.receiver {
            "required"
        } else if self.state.asking.is_some() {
            "pending"
        } else if refused {
            "denied"
        } else {
            self.state.asking = Some(device);
            "pending"
        };
        Ok(answer.into())
    }
}

impl remote::Engine for RemoteEngine {
    fn state(&self) -> &State {
        &self.state
    }

    fn wake(&mut self) {
        if !self.started {
            self.started = true;
            self.load();
            self.apply();
        } else {
            self.observe();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use remote::Commands;

    fn engine() -> RemoteEngine {
        RemoteEngine {
            db: Db::in_memory(),
            ..RemoteEngine::default()
        }
    }

    fn client(key: &str) -> String {
        serde_json::json!({ "key": key, "name": "手机", "platform": "ios", "address": "1.2.3.4" })
            .to_string()
    }

    #[test]
    fn a_device_is_asked_about_once_and_remembered_when_trusted() {
        let mut engine = engine();
        assert_eq!(engine.authorize(client("id:a"), false).unwrap(), "required");
        assert_eq!(engine.authorize(client("id:a"), true).unwrap(), "pending");
        assert_eq!(engine.authorize(client("id:b"), true).unwrap(), "pending");
        assert_eq!(engine.state.asking.as_ref().unwrap().key, "id:a");
        engine.answer(true, true).unwrap();
        assert_eq!(engine.authorize(client("id:a"), false).unwrap(), "authorized");
        assert_eq!(engine.state.trusted.len(), 1);
        engine.forget_device("id:a".into()).unwrap();
        assert_eq!(engine.authorize(client("id:a"), false).unwrap(), "required");
    }

    #[test]
    fn a_refused_device_waits() {
        let mut engine = engine();
        engine.authorize(client("id:a"), true).unwrap();
        engine.answer(false, false).unwrap();
        assert_eq!(engine.authorize(client("id:a"), true).unwrap(), "denied");
    }

    #[test]
    fn a_port_is_a_port() {
        let mut engine = engine();
        assert!(engine.set_port("0".into()).is_err());
        assert!(engine.set_port("70000".into()).is_err());
    }
}
