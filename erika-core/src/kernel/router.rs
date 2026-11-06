//! A native app whose WebUI is open has two kernels: liberika, for what this
//! device plays, and the browser's, for what a page plays. There is one
//! playback, as there is one engine: a video started from a page plays in that
//! page, and one started anywhere else plays here. Whichever plays takes every
//! other call, so a window can control what a page plays and the other way
//! round.

use super::{DanmakuConfig, Kernel, OperationResult, Playback};
use erika_protocol::erika::Enhancement;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct Router {
    native: Arc<dyn Kernel>,
    web: Arc<dyn Kernel>,
    /// What plays now is the page's.
    browser: AtomicBool,
}

fn from_page() -> bool {
    bern::current_entry() == Some(bern::Entry::Web)
}

impl Router {
    pub fn install(native: Arc<dyn Kernel>, web: Arc<dyn Kernel>) -> Arc<dyn Kernel> {
        Arc::new(Router {
            native,
            web,
            browser: AtomicBool::new(false),
        })
    }

    fn active(&self) -> &Arc<dyn Kernel> {
        if self.browser.load(Ordering::Relaxed) {
            &self.web
        } else {
            &self.native
        }
    }
}

impl Kernel for Router {
    fn is_browser(&self) -> bool {
        from_page() || self.browser.load(Ordering::Relaxed)
    }
    fn file_name(&self, url: &str) -> Option<String> {
        self.web.file_name(url).or_else(|| self.native.file_name(url))
    }
    /// A new source: where it plays is decided here.
    fn prepare_source(&self, url: &str) -> Result<(), String> {
        let browser = from_page();
        if browser != self.browser.swap(browser, Ordering::Relaxed) {
            // What played on the other side stops.
            let other = if browser { &self.native } else { &self.web };
            other.close();
        }
        self.active().prepare_source(url)
    }
    fn browser_file(&self, client: &str, url: &str, name: &str) -> Result<(), String> {
        self.web.browser_file(client, url, name)
    }
    fn browser_exchange(&self, client: &str, packet: &str) -> Result<String, String> {
        self.web.browser_exchange(client, packet)
    }
    fn ready(&self) -> bool {
        // Before a source is chosen, liberika's frontend answers.
        self.native.ready() || self.web.ready()
    }
    fn open(&self, url: &str, start: f64) {
        self.active().open(url, start);
    }
    fn play(&self) {
        self.active().play();
    }
    fn pause(&self) {
        self.active().pause();
    }
    fn seek(&self, seconds: f64) {
        self.active().seek(seconds);
    }
    fn set_volume(&self, volume: f64) {
        self.active().set_volume(volume);
    }
    fn set_rate(&self, rate: f64) {
        self.active().set_rate(rate);
    }
    fn set_hardware_decode(&self, enabled: bool) {
        self.active().set_hardware_decode(enabled);
    }
    fn select_audio(&self, id: i64) {
        self.active().select_audio(id);
    }
    fn select_subtitle(&self, id: Option<i64>) {
        self.active().select_subtitle(id);
    }
    fn add_subtitle(&self, url: &str) {
        self.active().add_subtitle(url);
    }
    fn set_subtitle_scale(&self, scale: f64) {
        self.active().set_subtitle_scale(scale);
    }
    fn set_subtitle_supersampling(&self, enabled: bool) {
        self.active().set_subtitle_supersampling(enabled);
    }
    fn set_enhancement(&self, enhancement: Enhancement) {
        self.active().set_enhancement(enhancement);
    }
    fn set_danmaku(&self, config: &DanmakuConfig) {
        self.active().set_danmaku(config);
    }
    fn add_danmaku(&self, name: &str, contents: &str) -> Result<usize, String> {
        self.active().add_danmaku(name, contents)
    }
    fn send_danmaku(&self, text: &str, rgb: u32) {
        self.active().send_danmaku(text, rgb);
    }
    fn mount_danmaku_file(&self, url: &str, name: &str) -> bool {
        self.active().mount_danmaku_file(url, name)
    }
    fn take_danmaku_result(&self) -> Option<(String, Result<usize, String>)> {
        self.active().take_danmaku_result()
    }
    fn captured_source(&self) -> Option<String> {
        self.active().captured_source()
    }
    fn capture(&self, path: &Path, width: Option<u32>) {
        self.active().capture(path, width);
    }
    fn close(&self) {
        self.active().close();
    }
    fn playback(&self) -> Playback {
        self.active().playback()
    }
    fn take_results(&self) -> Vec<OperationResult> {
        self.active().take_results()
    }
}
