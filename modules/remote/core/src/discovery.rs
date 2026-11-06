//! How NipaPlay finds a host on the network: a UDP broadcast on one port,
//! answered to its sender.

use serde_json::{Value, json};
use std::io;
use std::net::UdpSocket;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const PORT: u16 = 32123;

pub struct Discovery {
    stop: Arc<AtomicBool>,
}

impl Discovery {
    /// Answer as a host listening on `port`, until dropped. Fails while
    /// another program, such as NipaPlay itself, holds the port.
    pub fn start(port: u16) -> io::Result<Discovery> {
        let socket = UdpSocket::bind(("0.0.0.0", PORT))?;
        socket.set_read_timeout(Some(Duration::from_millis(500)))?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        std::thread::Builder::new()
            .name("erika-discovery".into())
            .spawn(move || {
                let mut buffer = [0; 1024];
                while !flag.load(Ordering::Relaxed) {
                    let Ok((length, sender)) = socket.recv_from(&mut buffer) else {
                        continue;
                    };
                    if asks(&buffer[..length]) {
                        let _ = socket.send_to(answer(port).to_string().as_bytes(), sender);
                    }
                }
            })?;
        Ok(Discovery { stop })
    }
}

impl Drop for Discovery {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn asks(packet: &[u8]) -> bool {
    serde_json::from_slice::<Value>(packet).is_ok_and(|value| {
        value["type"] == "nipaplay_discover" && value["v"] == 1
    })
}

fn answer(port: u16) -> Value {
    json!({
        "type": "nipaplay_discover_response",
        "v": 1,
        // What NipaPlay checks, to the letter.
        "app": "NipaPlay",
        "hostname": crate::share::hostname(),
        "os": std::env::consts::OS,
        "port": port,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_discover_packet_is_answered() {
        assert!(asks(br#"{"type":"nipaplay_discover","v":1}"#));
        assert!(!asks(br#"{"type":"nipaplay_discover","v":2}"#));
        assert!(!asks(b"hello"));
        assert_eq!(answer(1180)["port"], 1180);
        assert_eq!(answer(1180)["app"], "NipaPlay");
    }
}
