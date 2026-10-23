//! Browser adapter: no frame buffers cross the WASM or WebUI boundary.
//! Commands are copied by the JS extension before being deferred; snapshots
//! contain only playback metadata. liberika's worker owns all decoding/rendering.
use super::{Chapter, DanmakuConfig, Kernel, Playback, Track};
use erika_protocol::erika::Enhancement;
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "erika_web")]
unsafe extern "C" {
    fn ready() -> i32;
    fn request(ptr: *const u8, len: usize);
    fn snapshot(out: *mut u8, capacity: usize) -> usize;
    fn result(out: *mut u8, capacity: usize) -> usize;
    fn operation_result(out: *mut u8, capacity: usize) -> usize;
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use erika_protocol::erika::Commands;
    #[test]
    fn native_engine_plays_browser_files_and_follows_real_observations() {
        let kernel = Web::install();
        let mut engine = crate::ErikaEngine::default();
        engine.kernel = Some(kernel.clone());
        engine
            .browser_file("page".into(), "blob:local".into(), "本地影片.mkv".into())
            .unwrap();
        engine
            .open_file(crate::MediaSource::new("blob:local"))
            .unwrap();
        assert!(engine.state.video);
        assert_eq!(engine.state.title, "本地影片.mkv");
        let commands: Value =
            serde_json::from_str(&kernel.browser_exchange("page", "{}").unwrap()).unwrap();
        assert!(
            commands["commands"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["message"]["op"] == "open")
        );
        let packet = json!({"generation":commands["generation"],"report":1,"snapshot":{
            "source":"blob:local","loaded":true,"playing":true,"position":2.0,"duration":12.0,
            "video":"H264 · 240p · WebCodecs","audioTracks":[{"id":1,"name":"AAC"}]
        }});
        kernel
            .browser_exchange("page", &packet.to_string())
            .unwrap();
        engine.player_tick().unwrap();
        assert_eq!(engine.state.elapsed, 2.0);
        assert_eq!(engine.state.duration, 12.0);
        assert_eq!(engine.state.player.audio_tracks.len(), 1);
        engine.toggle_pause().unwrap();
        let commands: Value =
            serde_json::from_str(&kernel.browser_exchange("page", "{}").unwrap()).unwrap();
        assert!(
            commands["commands"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["message"]["op"] == "pause")
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod remote;

#[derive(Default)]
pub struct Web {
    #[cfg(not(target_arch = "wasm32"))]
    transport: std::sync::Mutex<remote::Transport>,
}
enum Read {
    Snapshot,
    Danmaku,
    Operation,
}
impl Web {
    pub fn install() -> Arc<dyn Kernel> {
        Arc::new(Self::default())
    }
    fn send(&self, value: Value) {
        #[cfg(not(target_arch = "wasm32"))]
        self.transport.lock().unwrap().send(value);
        #[cfg(target_arch = "wasm32")]
        self.send_wasm(value);
    }
    #[cfg(target_arch = "wasm32")]
    fn send_wasm(&self, value: Value) {
        let bytes = value.to_string();
        // The extension copies the string during this call.
        unsafe { request(bytes.as_ptr(), bytes.len()) }
    }
    fn read(&self, kind: Read) -> Value {
        #[cfg(not(target_arch = "wasm32"))]
        return self.transport.lock().unwrap().read(kind);
        #[cfg(target_arch = "wasm32")]
        {
            self.read_wasm(match kind {
                Read::Snapshot => snapshot,
                Read::Danmaku => result,
                Read::Operation => operation_result,
            })
        }
    }
    #[cfg(target_arch = "wasm32")]
    fn read_wasm(&self, import: unsafe extern "C" fn(*mut u8, usize) -> usize) -> Value {
        // Both calls are synchronous on the same browser thread.
        let len = unsafe { import(std::ptr::null_mut(), 0) };
        if len > 4 * 1024 * 1024 {
            return Value::Null;
        }
        let mut bytes = vec![0; len];
        let written = unsafe { import(bytes.as_mut_ptr(), bytes.len()) };
        if written != len {
            return Value::Null;
        }
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    }
}
impl Kernel for Web {
    fn is_browser(&self) -> bool {
        true
    }
    fn ready(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        return unsafe { ready() != 0 };
        #[cfg(not(target_arch = "wasm32"))]
        true
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn file_name(&self, url: &str) -> Option<String> {
        self.transport.lock().unwrap().file_name(url)
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn prepare_source(&self, url: &str) -> Result<(), String> {
        self.transport.lock().unwrap().prepare(url)
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn browser_file(&self, client: &str, url: &str, name: &str) -> Result<(), String> {
        self.transport.lock().unwrap().register(client, url, name)
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn browser_exchange(&self, client: &str, packet: &str) -> Result<String, String> {
        self.transport.lock().unwrap().exchange(client, packet)
    }
    fn open(&self, url: &str, start: f64) {
        self.send(json!({"op":"open", "url":url, "start":start}));
    }
    fn play(&self) {
        self.send(json!({"op":"play"}));
    }
    fn pause(&self) {
        self.send(json!({"op":"pause"}));
    }
    fn seek(&self, value: f64) {
        self.send(json!({"op":"seek", "value":value}));
    }
    fn set_volume(&self, value: f64) {
        self.send(json!({"op":"set_volume", "value":value}));
    }
    fn set_rate(&self, value: f64) {
        self.send(json!({"op":"rate", "value":value}));
    }
    fn set_hardware_decode(&self, value: bool) {
        self.send(json!({"op":"hardware", "value":value}));
    }
    fn select_audio(&self, value: i64) {
        self.send(json!({"op":"audio", "value":value}));
    }
    fn select_subtitle(&self, value: Option<i64>) {
        self.send(json!({"op":"subtitle", "value":value}));
    }
    fn add_subtitle(&self, url: &str) {
        self.send(json!({"op":"addSubtitle", "url":url}));
    }
    fn set_subtitle_scale(&self, value: f64) {
        self.send(json!({"op":"subtitleOptions", "config":{"scale":value}}));
    }
    fn set_subtitle_supersampling(&self, value: bool) {
        self.send(json!({"op":"subtitleOptions", "config":{"supersampling":value}}));
    }
    fn set_enhancement(&self, _: Enhancement) {} // Not exposed by the browser UI.
    fn set_danmaku(&self, c: &DanmakuConfig) {
        self.send(json!({"op":"danmakuOptions", "config":{
            "enabled":c.enabled, "supersampling":c.supersampling, "opacity":c.opacity,
            "fontSize":c.font_size, "area":c.area, "duration":c.scroll_seconds, "delay":c.delay,
            "mergeDuplicates":c.merge, "blockTop":c.block_top, "blockBottom":c.block_bottom, "blockScroll":c.block_scroll
        }}));
    }
    fn add_danmaku(&self, name: &str, contents: &str) -> Result<usize, String> {
        self.send(json!({"op":"addDanmaku", "name":name, "text":contents}));
        Ok(0) // Network matching already knows the count; parsing stays in the worker.
    }
    fn mount_danmaku_file(&self, url: &str, name: &str) -> bool {
        self.send(json!({"op":"addDanmaku", "name":name, "url":url, "notify":true}));
        true
    }
    fn take_danmaku_result(&self) -> Option<(String, Result<usize, String>)> {
        let v = self.read(Read::Danmaku);
        let name = v["name"].as_str()?.to_owned();
        Some((
            name,
            match v["error"].as_str() {
                Some(error) => Err(error.into()),
                None => Ok(v["count"].as_u64().unwrap_or(0) as usize),
            },
        ))
    }
    fn send_danmaku(&self, text: &str) {
        self.send(json!({"op":"sendDanmaku", "text":text}));
    }
    fn capture(&self, path: &Path, width: Option<u32>) {
        self.send(
            json!({"op":"capture", "name":path.file_name().map(|s|s.to_string_lossy()), "width":width}),
        );
    }
    fn take_results(&self) -> Vec<super::OperationResult> {
        let mut results = Vec::new();
        loop {
            let v = self.read(Read::Operation);
            let Some(op) = v["op"].as_str() else {
                break;
            };
            results.push(super::OperationResult {
                source: v["source"].as_str().map(str::to_owned),
                operation: match op {
                    "subtitle" => {
                        super::Operation::Subtitle(v["url"].as_str().unwrap_or("").into())
                    }
                    "screenshot" => super::Operation::Screenshot(
                        v["name"].as_str().unwrap_or("Erika.png").into(),
                    ),
                    _ => super::Operation::Setting,
                },
                result: v["error"].as_str().map_or(Ok(()), |e| Err(e.into())),
            });
        }
        results
    }
    fn captured_source(&self) -> Option<String> {
        self.read(Read::Snapshot)["still"]
            .as_str()
            .map(str::to_owned)
    }
    fn close(&self) {
        self.send(json!({"op":"close"}));
    }
    fn playback(&self) -> Playback {
        let v = self.read(Read::Snapshot);
        let tracks = |key: &str| {
            v[key]
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|t| {
                            Some(Track {
                                id: t["id"].as_i64()?,
                                name: t["name"].as_str()?.into(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        Playback {
            source: v["source"].as_str().map(str::to_owned),
            loaded: v["loaded"].as_bool().unwrap_or(false),
            playing: v["playing"].as_bool().unwrap_or(false),
            ended: v["ended"].as_bool().unwrap_or(false),
            picture: v["picture"].as_bool().unwrap_or(false),
            position: v["position"].as_f64().unwrap_or(0.0),
            duration: v["duration"].as_f64(),
            audio_tracks: tracks("audioTracks"),
            subtitle_tracks: tracks("subtitleTracks"),
            audio_track: v["audioTrack"].as_i64(),
            subtitle_track: v["subtitleTrack"].as_i64(),
            chapters: v["chapters"]
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|c| {
                            Some(Chapter {
                                title: c["title"].as_str()?.into(),
                                start: c["start"].as_f64()?,
                                end: c["end"].as_f64(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
            video: v["video"].as_str().unwrap_or("").into(),
            error: v["error"].as_str().map(str::to_owned),
            ..Playback::default()
        }
    }
}
