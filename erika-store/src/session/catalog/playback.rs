use super::*;
use erika_catalog::playback::Intervals;

impl Db {
    pub fn catalog_record_playback(
        &self,
        file: i64,
        expected: u64,
        intervals: &Intervals,
        duration: f64,
        ended: bool,
    ) -> Result<Vec<MediaId>> {
        if !duration.is_finite() || duration < 0. {
            return Err(Error::invalid("播放时长不正确"));
        }
        self.edit(|s| {
            if s.catalog.revision(file) != expected {
                return Err(Error::conflict("播放时的文件绑定已改变"));
            }
            let binding = s
                .catalog
                .bindings
                .get(&file)
                .ok_or_else(|| Error::conflict("播放文件尚未绑定"))?;
            let mut measured = s
                .catalog
                .played
                .get(&file)
                .filter(|(revision, _)| *revision == expected)
                .map(|(_, p)| p.clone())
                .unwrap_or_default();
            measured.merge(intervals)?;
            let completed = measured.completed(binding, duration, ended);
            let mut changed = Vec::new();
            for media in completed {
                let old = s.catalog.watched.get(&media);
                if old.is_some_and(|w| w.manual || w.watched) {
                    continue;
                }
                let revision = old.map_or(0, |w| w.revision);
                s.put_watched(&Watched {
                    media: media.clone(),
                    watched: true,
                    manual: false,
                    updated_at: crate::now(),
                    revision,
                })?;
                changed.push(media);
            }
            s.catalog.played.insert(file, (expected, measured));
            Ok(changed)
        })
    }
}
