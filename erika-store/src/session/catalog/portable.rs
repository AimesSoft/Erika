//! Browser snapshot import is one draft-state edit.
use super::*;
use crate::catalog::{CatalogSnapshot, PortableBinding};

impl State {
    fn export_catalog(&self) -> Result<CatalogSnapshot> {
        let key = |file| {
            self.files
                .get(&file)
                .and_then(Clone::clone)
                .ok_or_else(|| Error::invalid("文件缺少可同步的身份"))
        };
        let mut snapshot = CatalogSnapshot {
            nodes: self.catalog.nodes.values().cloned().collect(),
            links: self.catalog.links.clone(),
            watched: self.catalog.watched.values().cloned().collect(),
            marks: self.catalog.marks.clone(),
            ..Default::default()
        };
        for b in self.catalog.bindings.values() {
            let mut binding = b.clone();
            binding.file = 0;
            snapshot.bindings.push(PortableBinding {
                file: key(b.file)?,
                binding,
            });
        }
        for b in self.catalog.danmaku.values() {
            let mut binding = b.clone();
            binding.file = 0;
            snapshot.danmaku.push((key(b.file)?, binding));
        }
        Ok(snapshot)
    }
    fn import_catalog(&mut self, input: &CatalogSnapshot, replace: bool) -> Result<()> {
        let nodes: Vec<_> = self.catalog.nodes.values().cloned().collect();
        let snapshot = crate::catalog_snapshot::normalize(input, &nodes)?;
        let changed = crate::catalog_snapshot::changed_nodes(&snapshot.nodes, &nodes);
        self.catalog.put_nodes(&changed)?;
        let mut unique = HashSet::new();
        for input in &snapshot.links {
            if !unique.insert((input.media.clone(), input.reference.clone())) {
                return Err(Error::invalid("快照包含重复关联"));
            }
            let previous = self
                .catalog
                .links
                .iter()
                .find(|l| l.media == input.media && l.reference == input.reference);
            let mut value = input.clone();
            value.revision = previous.map_or(0, |l| l.revision);
            if let Some(previous) = previous {
                if previous == &value {
                    continue;
                }
                if previous.manual && !replace {
                    return Err(Error::conflict("远端关联与本地人工关联冲突"));
                }
                if replace && previous.manual {
                    value.manual = true;
                }
            }
            self.catalog.put_link(&value)?;
        }
        let mut unique = HashSet::new();
        for entry in &snapshot.bindings {
            if !unique.insert(&entry.file) {
                return Err(Error::invalid("快照包含重复文件绑定"));
            }
            let file = self.file(Some(&entry.file));
            let mut value = entry.binding.clone();
            value.file = file;
            value.revision = self.catalog.revision(file);
            if let Some(previous) = self.catalog.bindings.get(&file) {
                if previous.items == value.items
                    && previous.decision == value.decision
                    && previous.evidence == value.evidence
                {
                    continue;
                }
                if previous.decision != Decision::Auto && !replace {
                    return Err(Error::conflict("远端绑定与本地人工决定冲突"));
                }
                if replace
                    && previous.decision != Decision::Auto
                    && value.decision == Decision::Auto
                {
                    value.decision = Decision::Manual;
                }
            }
            self.put_binding(&value)?;
        }
        let mut unique = HashSet::new();
        for (key, input) in &snapshot.danmaku {
            if !unique.insert(key) {
                return Err(Error::invalid("快照包含重复弹幕关联"));
            }
            let file = self.file(Some(key));
            let mut value = input.clone();
            value.file = file;
            value.revision = self.catalog.danmaku_revision(file);
            if let Some(previous) = self.catalog.danmaku.get(&file) {
                if previous == &value {
                    continue;
                }
                if previous.manual && !replace {
                    return Err(Error::conflict("远端弹幕关联与本地人工决定冲突"));
                }
                if replace && previous.manual {
                    value.manual = true;
                }
            }
            self.put_danmaku(&value)?;
        }
        let mut unique = HashSet::new();
        for input in &snapshot.watched {
            if !unique.insert(&input.media) {
                return Err(Error::invalid("快照包含冲突的观看记录"));
            }
            let mut value = input.clone();
            let previous = self.catalog.watched.get(&value.media);
            value.revision = previous.map_or(0, |w| w.revision);
            if let Some(previous) = previous {
                if previous == &value
                    || (!replace
                        && previous.watched == value.watched
                        && previous.manual > value.manual)
                {
                    continue;
                }
                if previous.manual && !replace {
                    return Err(Error::conflict("远端观看记录与本地人工决定冲突"));
                }
                if !replace && previous.updated_at > value.updated_at {
                    continue;
                }
                if replace && previous.manual {
                    value.manual = true;
                }
            }
            self.put_watched(&value)?;
        }
        let mut unique = HashSet::new();
        for value in &snapshot.marks {
            if !unique.insert((&value.media, &value.reference)) {
                return Err(Error::invalid("快照包含重复作品状态"));
            }
            let previous = self
                .catalog
                .marks
                .iter()
                .find(|m| m.media == value.media && m.reference == value.reference);
            if let Some(value) = value.imported(previous, replace)? {
                self.put_mark(&value)?;
            }
        }
        Ok(())
    }
}
impl Db {
    pub fn catalog_export(&self) -> Result<CatalogSnapshot> {
        self.read(State::export_catalog)
    }
    pub fn catalog_import_checked(
        &self,
        snapshot: &CatalogSnapshot,
        expected: &CatalogSnapshot,
        removed_bindings: &[crate::portable::FileKey],
        removed_danmaku: &[crate::portable::FileKey],
    ) -> Result<()> {
        self.edit(|s| {
            if &s.export_catalog()? != expected {
                return Err(Error::conflict("媒体库在同步期间已改变，请重新同步"));
            }
            for key in removed_bindings {
                let id = s.file(Some(key));
                s.catalog.bindings.remove(&id);
                s.catalog.bump(id)?;
                s.links.remove(&id);
            }
            for key in removed_danmaku {
                let id = s.file(Some(key));
                s.catalog.danmaku.remove(&id);
                let revision = next(s.catalog.danmaku_revision(id))?;
                s.catalog.danmaku_revisions.insert(id, revision);
            }
            for entry in &snapshot.bindings {
                if entry.binding.decision == Decision::Auto {
                    let id = s.file(Some(&entry.file));
                    if s.catalog
                        .bindings
                        .get(&id)
                        .is_some_and(|b| b.decision != Decision::Auto)
                    {
                        s.catalog.bindings.remove(&id);
                    }
                }
            }
            for (key, entry) in &snapshot.danmaku {
                if !entry.manual {
                    let id = s.file(Some(key));
                    if s.catalog.danmaku.get(&id).is_some_and(|b| b.manual) {
                        s.catalog.danmaku.remove(&id);
                    }
                }
            }
            s.import_catalog(snapshot, true)
        })
    }
    pub fn catalog_import(
        &self,
        snapshot: &CatalogSnapshot,
        replace_conflicts: bool,
    ) -> Result<()> {
        self.edit(|s| s.import_catalog(snapshot, replace_conflicts))
    }
}
