//! Measured playback intervals, not the position of the seek slider. Stored per
//! physical file and binding generation; alternate encodes never share seconds.
use crate::{FileBinding, MediaId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Intervals(Vec<[f64; 2]>);

impl Intervals {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn add(&mut self, start: f64, end: f64) -> crate::Result<()> {
        self.merge(&Self(vec![[start, end]]))
    }
    /// Coalesce overlaps without double-counting repeats or filling seek gaps.
    pub fn merge(&mut self, other: &Self) -> crate::Result<()> {
        if self.0.len() > 8192 || other.0.len() > 8192 {
            return Err(crate::Error::invalid("播放区间过多"));
        }
        let mut rows = self.0.clone();
        rows.extend_from_slice(&other.0);
        if rows
            .iter()
            .any(|[a, b]| !a.is_finite() || !b.is_finite() || *a < 0. || b <= a)
        {
            return Err(crate::Error::invalid("播放区间不正确"));
        }
        rows.sort_by(|a, b| a[0].total_cmp(&b[0]));
        let mut merged: Vec<[f64; 2]> = Vec::new();
        for [start, end] in rows {
            if let Some(last) = merged.last_mut()
                && start <= last[1]
            {
                last[1] = last[1].max(end);
            } else {
                merged.push([start, end]);
            }
        }
        if merged.len() > 8192 {
            return Err(crate::Error::invalid("播放区间过多"));
        }
        self.0 = merged;
        Ok(())
    }
    pub fn complete(&self, start: f64, end: f64) -> bool {
        if !start.is_finite() || !end.is_finite() || start < 0. || end <= start {
            return false;
        }
        let played: f64 = self
            .0
            .iter()
            .map(|[a, b]| (b.min(end) - a.max(start)).max(0.))
            .sum();
        played >= (end - start) * 0.9
    }
    pub fn completed(&self, binding: &FileBinding, duration: f64, ended: bool) -> Vec<MediaId> {
        if !duration.is_finite() || duration <= 0. {
            return Vec::new();
        }
        if !binding.has_item_positions() {
            // No invented division into equal-length episodes. Only an actual
            // EOF after playing the collection can complete its whole coverage.
            return if ended && self.complete(0., duration) {
                binding.items.iter().map(|i| i.media.clone()).collect()
            } else {
                Vec::new()
            };
        }
        binding
            .items
            .iter()
            .filter(|item| {
                let start = item.start_seconds.unwrap_or(0.);
                let end = binding.item_end(item).unwrap_or(duration);
                end <= duration && self.complete(start, end)
            })
            .map(|item| item.media.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeats_do_not_fill_seek_gaps_and_invalid_input_is_atomic() {
        let mut played = Intervals::default();
        played.add(0., 40.).unwrap();
        played.add(10., 40.).unwrap();
        played.add(80., 100.).unwrap();
        assert!(!played.complete(0., 100.));
        let before = played.clone();
        for (a, b) in [(0., f64::NAN), (-1., 1.), (2., 1.), (1., f64::INFINITY)] {
            assert!(played.add(a, b).is_err());
            assert_eq!(played, before);
        }
        played.add(40., 70.).unwrap();
        assert!(played.complete(0., 100.));
        assert!(!played.complete(0., 0.));
    }
}
