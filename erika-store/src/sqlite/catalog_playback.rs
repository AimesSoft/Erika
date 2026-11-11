//! A measured interval and its resulting watched marks commit together. These
//! local measurements are not remote account observations or resume positions.
use super::{
    Db,
    catalog::{binding, decode, json, put_watched, revision, transaction},
};
use crate::catalog::{Error, Result};
use erika_catalog::{MediaId, Watched, playback::Intervals};
use rusqlite::{OptionalExtension, params};

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
        let tx = transaction(&self.conn)?;
        if revision(&tx, file)? != expected {
            return Err(Error::conflict("播放时的文件绑定已改变"));
        }
        let binding = binding(&tx, file)?.ok_or_else(|| Error::conflict("播放文件尚未绑定"))?;
        let previous: Option<String> = tx
            .query_row(
                "SELECT intervals FROM media_playback WHERE file_id=?1 AND binding_revision=?2",
                params![file, expected as i64],
                |r| r.get(0),
            )
            .optional()?;
        let mut measured: Intervals = previous.map(decode).transpose()?.unwrap_or_default();
        measured.merge(intervals)?;
        let mut changed = Vec::new();
        for media in measured.completed(&binding, duration, ended) {
            let old = self.catalog_watched(&media)?;
            if old.as_ref().is_some_and(|w| w.manual || w.watched) {
                continue;
            }
            put_watched(
                &tx,
                &Watched {
                    media: media.clone(),
                    watched: true,
                    manual: false,
                    updated_at: crate::now(),
                    revision: old.map_or(0, |w| w.revision),
                },
            )?;
            changed.push(media);
        }
        tx.execute("INSERT INTO media_playback(file_id,binding_revision,intervals) VALUES(?1,?2,?3) ON CONFLICT(file_id) DO UPDATE SET binding_revision=excluded.binding_revision,intervals=excluded.intervals",
            params![file,expected as i64,json(&measured)?])?;
        tx.commit()?;
        Ok(changed)
    }
}
