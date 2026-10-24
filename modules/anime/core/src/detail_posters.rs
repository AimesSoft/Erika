//! Public Bangumi large covers, downloaded separately from details. A failed
//! download never replaces the original poster or blocks the details page.
use super::{Done, in_background};
use bern::media::MediaSource;
use erika_dandanplay::{Details, bangumi, reviews};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct Key {
    subject: i64,
    original: String,
}

impl Key {
    fn of(details: &Details) -> Option<Self> {
        // Never use the numerically similar dandanplay ID as a Bangumi ID.
        let subject = bangumi::subject_id(details.anime_id)
            .or_else(|| reviews::subject_id(&details.bangumi_url))?;
        Some(Self {
            subject,
            original: details.image_url.clone(),
        })
    }

    fn base(&self, root: &Path) -> PathBuf {
        // A changed upstream cover gets its own cache and cannot be overwritten
        // by an older request for the same show.
        root.join(format!(
            "bangumi-{}-{:x}",
            self.subject,
            Sha256::digest(self.original.as_bytes())
        ))
    }

    fn cached_at(&self, root: &Path) -> Option<PathBuf> {
        ["jpg", "png", "webp"]
            .into_iter()
            .map(|ext| self.base(root).with_extension(ext))
            .find(|path| path.is_file())
    }
}

fn directory() -> Option<PathBuf> {
    Some(bern::storage::cache_dir()?.join("detail-posters"))
}

pub(super) fn cached(details: &Details) -> Option<MediaSource> {
    Some(MediaSource::file(
        Key::of(details)?.cached_at(&directory()?)?,
    ))
}

#[cfg(not(target_arch = "wasm32"))]
fn save(root: &Path, key: &Key, bytes: &[u8]) -> Option<PathBuf> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let extension = match reader.format()? {
        image::ImageFormat::Jpeg => "jpg",
        image::ImageFormat::Png => "png",
        image::ImageFormat::WebP => "webp",
        _ => return None,
    };
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(10_000);
    limits.max_image_height = Some(10_000);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let decoded = reader.decode().ok()?;
    // Do not exchange the standard 400px cover for a thumbnail or placeholder.
    if decoded.width().max(decoded.height()) <= 400 {
        return None;
    }
    std::fs::create_dir_all(root).ok()?;
    let path = key.base(root).with_extension(extension);
    let temporary = key.base(root).with_extension("tmp");
    std::fs::write(&temporary, bytes).ok()?;
    std::fs::rename(temporary, &path).ok()?;
    Some(path)
}

#[cfg(not(target_arch = "wasm32"))]
fn download(
    root: &Path,
    key: &Key,
    fetch: impl FnOnce(i64) -> erika_dandanplay::Result<Vec<u8>>,
) -> Option<PathBuf> {
    key.cached_at(root)
        .or_else(|| save(root, key, &fetch(key.subject).ok()?))
}

#[derive(Default)]
pub(super) struct Downloads {
    pending: HashSet<Key>,
}

impl Downloads {
    pub fn request(&mut self, details: &Details, inbox: &Arc<Mutex<Vec<Done>>>) {
        if cfg!(target_arch = "wasm32") {
            return;
        }
        let Some(key) = Key::of(details) else { return };
        let Some(root) = directory() else { return };
        if key.cached_at(&root).is_some() || !self.pending.insert(key.clone()) {
            return;
        }
        in_background(inbox, move || {
            #[cfg(not(target_arch = "wasm32"))]
            download(&root, &key, bangumi::poster);
            Some(Done::DetailPoster(key))
        });
    }

    pub fn finished(&mut self, key: &Key) {
        self.pending.remove(key);
    }

    pub fn matches(key: &Key, details: &Details) -> bool {
        Key::of(details).as_ref() == Some(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn details(id: i64, link: &str) -> Details {
        serde_json::from_value(serde_json::json!({
            "animeId": id, "imageUrl": "https://example.com/standard.jpg",
            "bangumiUrl": link,
        }))
        .unwrap()
    }

    #[test]
    fn cover_mapping_uses_verified_subjects_and_tracks_original_changes() {
        assert!(Key::of(&details(17527, "")).is_none());
        assert!(Key::of(&details(17527, "https://example.com/subject/393010")).is_none());
        let original = details(17527, "https://bgm.tv/subject/393010");
        let key = Key::of(&original).unwrap();
        assert_eq!(key.subject, 393010);
        assert_eq!(
            Key::of(&details(bangumi::local_id(393010).unwrap(), ""))
                .unwrap()
                .subject,
            393010
        );
        let mut changed = original.clone();
        changed.image_url = "https://example.com/replacement.jpg".into();
        assert!(!Downloads::matches(&key, &changed));
        assert_ne!(
            key.base(Path::new("cache")),
            Key::of(&changed).unwrap().base(Path::new("cache"))
        );
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn failed_or_small_cover_keeps_original_and_verified_cover_is_reused_offline() {
        let root = std::env::temp_dir().join(format!(
            "erika-hd-poster-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let key = Key::of(&details(17527, "https://bgm.tv/subject/393010")).unwrap();
        assert!(
            download(&root, &key, |_| Err(erika_dandanplay::Error {
                message: "offline".into(),
                sign_in: false
            }))
            .is_none()
        );
        assert!(save(&root, &key, b"<html>failed</html>").is_none());
        let png = |w, h| {
            let mut bytes = std::io::Cursor::new(Vec::new());
            image::DynamicImage::new_rgb8(w, h)
                .write_to(&mut bytes, image::ImageFormat::Png)
                .unwrap();
            bytes.into_inner()
        };
        assert!(save(&root, &key, &png(200, 300)).is_none());
        let bytes = png(800, 1200);
        assert!(save(&root, &key, &bytes[..bytes.len() / 2]).is_none());
        assert!(key.cached_at(&root).is_none());
        let path = download(&root, &key, |_| Ok(bytes.clone())).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(
            download(&root, &key, |_| panic!(
                "cached cover must not need the network"
            )),
            Some(path)
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn late_cover_for_another_show_does_not_touch_current_detail() {
        let mut engine = crate::AnimeEngine::default();
        let a = details(1, "https://bgm.tv/subject/393010");
        let b = details(2, "https://bgm.tv/subject/456078");
        let key_a = Key::of(&a).unwrap();
        let key_b = Key::of(&b).unwrap();
        engine.details.insert(1, a);
        engine.details.insert(2, b.clone());
        engine.state.opened = 2;
        engine.show_opened();
        let before = engine.state.show.clone();
        engine.inbox.lock().unwrap().push(Done::DetailPoster(key_a));
        engine.take_done();
        assert_eq!(engine.state.show, before);
        // A failed download for the current show also preserves its original.
        engine.inbox.lock().unwrap().push(Done::DetailPoster(key_b));
        engine.take_done();
        assert_eq!(engine.state.show.poster.url, b.image_url);
        assert!(!engine.state.details_failed);
    }
}
