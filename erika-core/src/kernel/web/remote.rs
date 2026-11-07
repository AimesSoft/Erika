//! Native WebUI command mailbox. Only metadata crosses the authenticated
//! channel; browser Files and decoded frames remain in the owning page.
use super::{Read, Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    time::{Duration, Instant},
};

#[derive(Default)]
pub(super) struct Transport {
    files: BTreeMap<String, (String, String)>,
    clients: BTreeMap<String, Instant>,
    owner: Option<String>,
    source: Option<String>,
    generation: u64,
    next: u64,
    report: u64,
    pending: VecDeque<Value>,
    snapshot: Value,
    results: VecDeque<Value>,
    operations: VecDeque<Value>,
}

impl Transport {
    fn client(client: &str) -> Result<(), String> {
        if client.is_empty() || client.len() > 128 {
            Err("Invalid browser client".into())
        } else {
            Ok(())
        }
    }
    pub fn register(&mut self, client: &str, url: &str, name: &str) -> Result<(), String> {
        Self::client(client)?;
        if !url.starts_with("blob:") || url.len() > 4096 || name.is_empty() || name.len() > 4096 {
            return Err("Invalid browser file".into());
        }
        if self.files.len() >= 1024 && !self.files.contains_key(url) {
            return Err("Too many browser files; reopen the page".into());
        }
        self.files.insert(url.into(), (client.into(), name.into()));
        self.clients.insert(client.into(), Instant::now());
        Ok(())
    }
    pub fn file_name(&self, url: &str) -> Option<String> {
        self.files.get(url).map(|(_, name)| name.clone())
    }
    pub fn prepare(&mut self, url: &str) -> Result<(), String> {
        let owner = if url.starts_with("blob:") {
            self.files
                .get(url)
                .map(|(client, _)| client.clone())
                .ok_or("请重新选择本地视频文件")?
        } else {
            self.owner.clone().ok_or("请先连接 WebUI 页面")?
        };
        if !self
            .clients
            .get(&owner)
            .is_some_and(|seen| seen.elapsed() < Duration::from_secs(10))
        {
            return Err("视频所在的网页已断开，请在当前网页重新选择文件".into());
        }
        self.owner = Some(owner);
        self.source = Some(url.into());
        self.generation += 1;
        self.report = 0;
        self.pending.clear();
        self.results.clear();
        self.operations.clear();
        self.snapshot = json!({"source":url});
        Ok(())
    }
    pub fn send(&mut self, message: Value) {
        if self.pending.len() >= 256 {
            self.snapshot =
                json!({"source":self.source, "error":"Browser playback command queue is full"});
            return;
        }
        self.next += 1;
        self.pending
            .push_back(json!({"id":self.next,"message":message}));
    }
    pub fn exchange(&mut self, client: &str, packet: &str) -> Result<String, String> {
        Self::client(client)?;
        if packet.len() > 1024 * 1024 {
            return Err("Browser report is too large".into());
        }
        let packet: Value = serde_json::from_str(packet).map_err(|e| e.to_string())?;
        if !packet.is_object() {
            return Err("Invalid browser report".into());
        }
        self.clients
            .retain(|_, seen| seen.elapsed() < Duration::from_secs(60));
        self.clients.insert(client.into(), Instant::now());
        // A page that reloads comes back as a new client.
        let owner_left = self.owner.as_ref().is_none_or(|owner| {
            self.clients
                .get(owner)
                .is_none_or(|seen| seen.elapsed() > Duration::from_secs(10))
        });
        if owner_left {
            self.owner = Some(client.into());
        }
        if self.owner.as_deref() != Some(client) {
            return Ok(json!({"active":false,"commands":[]}).to_string());
        }
        let ack = packet["ack"].as_u64().unwrap_or(0).min(self.next);
        self.pending
            .retain(|command| command["id"].as_u64().unwrap() > ack);
        let report = packet["report"].as_u64().unwrap_or(0);
        if packet["generation"].as_u64() == Some(self.generation) && report > self.report {
            self.report = report;
            let snapshot = &packet["snapshot"];
            if snapshot.is_object() && snapshot["source"].as_str() == self.source.as_deref() {
                self.snapshot = snapshot.clone();
                for (key, queue) in [
                    ("results", &mut self.results),
                    ("operations", &mut self.operations),
                ] {
                    if let Some(values) = packet[key].as_array() {
                        queue.extend(values.iter().take(128).cloned());
                        while queue.len() > 256 {
                            queue.pop_front();
                        }
                    }
                }
            }
        }
        Ok(json!({"active":true,"generation":self.generation,"commands":self.pending}).to_string())
    }
    pub fn read(&mut self, kind: Read) -> Value {
        match kind {
            Read::Snapshot => {
                if self.source.is_some()
                    && self
                        .owner
                        .as_ref()
                        .and_then(|owner| self.clients.get(owner))
                        .is_none_or(|seen| seen.elapsed() > Duration::from_secs(10))
                {
                    return json!({"source":self.source,"error":"视频所在的网页连接已断开"});
                }
                self.snapshot.clone()
            }
            Read::Danmaku => self.results.pop_front().unwrap_or(Value::Null),
            Read::Operation => self.operations.pop_front().unwrap_or(Value::Null),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_ownership_acknowledgements_and_stale_reports() {
        let mut t = Transport::default();
        t.register("one", "blob:one", "视频.mkv").unwrap();
        t.register("two", "blob:two", "next.webm").unwrap();
        t.prepare("blob:one").unwrap();
        t.send(json!({"op":"open", "url":"blob:one"}));
        let exchange = |t: &mut Transport, client, packet: Value| -> Value {
            serde_json::from_str(&t.exchange(client, &packet.to_string()).unwrap()).unwrap()
        };
        assert_eq!(exchange(&mut t, "two", json!({}))["active"], false);
        assert_eq!(
            exchange(&mut t, "one", json!({}))["commands"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let report = json!({"generation":1,"report":1,"ack":1,"snapshot":{"source":"blob:one","loaded":true,"position":3},"operations":[{"op":"screenshot","name":"one.png"}]});
        assert!(
            exchange(&mut t, "one", report.clone())["commands"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        exchange(&mut t, "one", report.clone());
        assert_eq!(
            t.operations.len(),
            1,
            "retried reports must not duplicate results"
        );
        assert_eq!(t.read(Read::Snapshot)["position"], 3);
        t.prepare("blob:two").unwrap();
        exchange(&mut t, "one", report);
        exchange(
            &mut t,
            "two",
            json!({"generation":1,"report":100,"snapshot":{"source":"blob:two","position":99}}),
        );
        assert!(t.read(Read::Snapshot)["position"].is_null());
        assert_eq!(t.file_name("blob:one").as_deref(), Some("视频.mkv"));
        assert!(t.prepare("blob:missing").is_err());
        t.clients
            .insert("two".into(), Instant::now() - Duration::from_secs(11));
        assert!(t.read(Read::Snapshot)["error"].is_string());
        assert!(t.prepare("blob:two").is_err());
    }
}
