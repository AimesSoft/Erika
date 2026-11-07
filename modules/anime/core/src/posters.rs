//! Keep public server posters locally: they never change, so a file shows them
//! at once and without the network.
use super::{Done, MediaSource, in_background};
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

fn name(url: &str) -> Option<&str> {
    let name = url
        .strip_prefix("http://43.142.85.190/dandanplay/public/images/")
        .or_else(|| {
            url.strip_prefix("https://nipaplay.aimes-soft.com/dandanplay/public/images/")
        })?;
    let hash = name.strip_suffix(".jpg")?;
    (hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())).then_some(name)
}

fn path(url: &str) -> Option<PathBuf> {
    Some(
        bern::storage::cache_dir()?
            .join("anime-posters")
            .join(name(url)?),
    )
}

pub fn source(url: &str) -> MediaSource {
    path(url)
        .filter(|p| p.is_file())
        .map(MediaSource::file)
        .unwrap_or_else(|| MediaSource::new(url))
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch(url: &str) -> Option<()> {
    let path = path(url)?;
    if path.is_file() {
        return Some(());
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(15)))
        .build()
        .into();
    let mut response = agent.get(url).call().ok()?;
    if !response
        .headers()
        .get("content-type")?
        .to_str()
        .ok()?
        .starts_with("image/")
    {
        return None;
    }
    let bytes = response
        .body_mut()
        .with_config()
        .limit(12 * 1024 * 1024)
        .read_to_vec()
        .ok()?;
    if bytes.is_empty() {
        return None;
    }
    std::fs::create_dir_all(path.parent()?).ok()?;
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, bytes).ok()?;
    std::fs::rename(temporary, path).ok()
}

#[derive(Default)]
pub(super) struct Downloads {
    queue: VecDeque<String>,
    pending: HashSet<String>,
    active: usize,
}

impl Downloads {
    pub fn add(&mut self, urls: impl Iterator<Item = String>, inbox: &Arc<Mutex<Vec<Done>>>) {
        if cfg!(target_arch = "wasm32") {
            return;
        }
        for url in urls {
            if path(&url).is_some_and(|p| !p.is_file()) && self.pending.insert(url.clone()) {
                self.queue.push_back(url);
            }
        }
        self.start(inbox);
    }

    pub fn finished(&mut self, url: &str, inbox: &Arc<Mutex<Vec<Done>>>) {
        self.pending.remove(url);
        self.active = self.active.saturating_sub(1);
        self.start(inbox);
    }

    fn start(&mut self, inbox: &Arc<Mutex<Vec<Done>>>) {
        while self.active < 4 {
            let Some(url) = self.queue.pop_front() else {
                break;
            };
            self.active += 1;
            in_background(inbox, move || {
                #[cfg(not(target_arch = "wasm32"))]
                let _ = fetch(&url);
                Some(Done::Poster(url))
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_server_owned_immutable_posters_enter_the_local_cache() {
        let hash = "a".repeat(64);
        assert_eq!(
            name(&format!(
                "http://43.142.85.190/dandanplay/public/images/{hash}.jpg"
            )),
            Some(format!("{hash}.jpg").as_str())
        );
        for url in [
            "https://example.com/image.jpg",
            "http://43.142.85.190/dandanplay/public/images/../cache.json",
            "http://43.142.85.190/dandanplay/public/images/x.jpg",
        ] {
            assert_eq!(name(url), None);
            assert_eq!(source(url).url, url);
        }
    }
}
