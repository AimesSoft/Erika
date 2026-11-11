use super::{Address, Db, State};
use crate::{
    catalog::{Error, Result},
    plugin_sources::{self as model, Descriptor, Item, Scan, StoredItem},
};
use std::collections::BTreeMap;

#[derive(Clone)]
pub(super) struct Directory {
    owner: String,
    descriptor: Descriptor,
    generation: String,
    scan: Option<Scan>,
    pages: BTreeMap<u32, String>,
    staged: BTreeMap<String, Item>,
    items: BTreeMap<String, StoredItem>,
}
impl State {
    pub(super) fn register_source(
        &mut self, owner: &str, descriptor: &Descriptor, added: i64,
    ) -> Result<(i64, bool)> {
        descriptor.validate(owner)?;
        let old = self.plugin_sources.iter()
            .find(|(_, d)| d.owner == owner && d.descriptor.id == descriptor.id)
            .map(|(id, _)| *id);
        if let Some(id) = old {
            let d = self.plugin_sources.get_mut(&id).unwrap();
            if d.descriptor.server_id != descriptor.server_id {
                return Err(Error::conflict("同一来源不能更换服务器身份"));
            }
            d.descriptor = descriptor.clone();
            let source = self.sources.get_mut(&id)
                .ok_or_else(|| Error::conflict("来源已删除"))?;
            source.name = descriptor.name.clone();
            source.content = descriptor.content.clone();
            return Ok((id, false));
        }
        self.next_source = self.next_source.max(1) + 1;
        let id = self.next_source;
        let mut source = descriptor.portable(owner);
        source.added = added;
        self.sources.insert(id, source);
        self.plugin_sources.insert(id, Directory {
            owner: owner.into(), descriptor: descriptor.clone(),
            generation: String::new(), scan: None,
            pages: BTreeMap::new(), staged: BTreeMap::new(), items: BTreeMap::new(),
        });
        Ok((id, true))
    }
    fn owned_source(&self, owner: &str, source: i64) -> Result<&Directory> {
        self.plugin_sources
            .get(&source)
            .filter(|d| d.owner == owner && self.sources.contains_key(&source))
            .ok_or_else(|| Error::conflict("来源不存在或不属于此插件"))
    }
    fn owned_scan(&self, owner: &str, id: &str) -> Result<(Scan, String)> {
        let d = self
            .plugin_sources
            .values()
            .find(|d| d.owner == owner && d.scan.as_ref().is_some_and(|s| s.id == id))
            .ok_or_else(|| Error::conflict("扫描已失效或不属于此插件"))?;
        let scan = d.scan.clone().unwrap();
        self.owned_source(owner, scan.source)?;
        Ok((scan, d.descriptor.server_id.clone()))
    }
    pub(super) fn plugin_item(&self, url: &str) -> Option<StoredItem> {
        self.plugin_sources
            .iter()
            .filter(|(id, _)| self.sources.contains_key(id))
            .flat_map(|(_, d)| d.items.values())
            .find(|i| i.url == url)
            .cloned()
    }
}
impl Db {
    pub fn plugin_source_ids(&self, owner: &str) -> Result<Vec<i64>> {
        Ok(self.read(|s| {
            s.plugin_sources
                .iter()
                .filter(|(id, d)| d.owner == owner && s.sources.contains_key(id))
                .map(|(id, _)| *id)
                .collect()
        }))
    }
    pub fn plugin_register_source(&self, owner: &str, descriptor: &Descriptor) -> Result<i64> {
        self.put_source(&descriptor.portable(owner)).map(|(id, _)| id)
    }
    pub fn plugin_begin_scan(&self, owner: &str, source: i64) -> Result<Scan> {
        self.edit(|s| {
            s.owned_source(owner, source)?;
            let scan = Scan::new(source);
            let d = s.plugin_sources.get_mut(&source).unwrap();
            d.scan = Some(scan.clone());
            d.pages.clear();
            d.staged.clear();
            Ok(scan)
        })
    }
    pub fn plugin_upsert_items(
        &self,
        owner: &str,
        id: &str,
        page: u32,
        items: &[Item],
    ) -> Result<Scan> {
        self.edit(|s| {
            let (mut scan, server) = s.owned_scan(owner, id)?;
            scan.active()?;
            let data = model::validate_page(owner, &server, items)?;
            let d = s.plugin_sources.get_mut(&scan.source).unwrap();
            if let Some(previous) = d.pages.get(&page) {
                if previous != &data {
                    return Err(Error::conflict("同一分页重试的内容不一致"));
                }
                return Ok(scan);
            }
            if page != scan.next_page {
                return Err(Error::conflict("扫描分页不连续"));
            }
            if page >= model::MAX_PAGES {
                return Err(Error::invalid("来源扫描分页过多"));
            }
            for item in items {
                let slot = item.slot();
                if let Some(previous) = d.staged.get(&slot) {
                    if previous != item {
                        return Err(Error::conflict("扫描中同一条目的内容互相冲突"));
                    }
                } else {
                    d.staged.insert(slot, item.clone());
                }
            }
            if d.staged.len() > model::MAX_ITEMS {
                return Err(Error::invalid("来源条目过多"));
            }
            scan.next_page += 1;
            scan.expires_at = crate::now() + model::LEASE_SECONDS;
            d.pages.insert(page, data);
            d.scan = Some(scan.clone());
            Ok(scan)
        })
    }
    pub fn plugin_finish_scan(
        &self,
        owner: &str,
        id: &str,
        pages: u32,
        complete: bool,
    ) -> Result<Scan> {
        self.edit(|s| {
            let (mut scan, _) = s.owned_scan(owner, id)?;
            if !scan.finish(pages, complete)? {
                return Ok(scan);
            }
            if complete {
                let staged = s.plugin_sources[&scan.source].staged.clone();
                let external = s.plugin_sources[&scan.source].descriptor.id.clone();
                let mut items = BTreeMap::new();
                for (slot, item) in staged {
                    let file = s.file(Some(&item.key(owner)?));
                    let url = model::address(owner, &external, &item.server_id, &slot);
                    if let Some(a) = s
                        .addresses
                        .iter_mut()
                        .find(|a| a.source == scan.source && a.url == url)
                    {
                        a.file = Some(file);
                        a.seen = crate::now();
                    } else {
                        s.next_address += 1;
                        s.addresses.push(Address {
                            key: s.next_address,
                            source: scan.source,
                            url: url.clone(),
                            file: Some(file),
                            size: 0,
                            modified: 0,
                            seen: crate::now(),
                        });
                    }
                    items.insert(
                        slot,
                        StoredItem {
                            source: scan.source,
                            owner: owner.into(),
                            generation: id.into(),
                            file,
                            url,
                            item,
                        },
                    );
                }
                let urls: std::collections::HashSet<_> =
                    items.values().map(|i| i.url.as_str()).collect();
                s.addresses
                    .retain(|a| a.source != scan.source || urls.contains(a.url.as_str()));
                let d = s.plugin_sources.get_mut(&scan.source).unwrap();
                d.items = items;
                d.generation = id.into();
            }
            let d = s.plugin_sources.get_mut(&scan.source).unwrap();
            d.scan = Some(scan.clone());
            d.staged.clear();
            d.pages.clear();
            Ok(scan)
        })
    }
    pub fn plugin_source_item(&self, url: &str) -> Result<Option<StoredItem>> {
        Ok(self.read(|s| s.plugin_item(url)))
    }
    pub fn plugin_source_page(
        &self,
        owner: &str,
        request: &model::page::ListRequest,
    ) -> Result<model::page::Page> {
        self.read(|s| {
            let d = s.owned_source(owner, request.source)?;
            let after = request.after(owner, &d.generation)?;
            let rows = d
                .items
                .range((std::ops::Bound::Excluded(after), std::ops::Bound::Unbounded))
                .take(request.limit + 1)
                .map(|(_, item)| Ok(item.clone()));
            model::page::collect(owner, request, &d.generation, rows)
        })
    }
    pub fn plugin_source_version(
        &self,
        owner: &str,
        request: &model::page::ItemRequest,
    ) -> Result<Option<StoredItem>> {
        let slot = request.slot()?;
        self.read(|s| {
            Ok(s.owned_source(owner, request.source)?
                .items
                .get(&slot)
                .cloned())
        })
    }
    pub fn plugin_source_status(
        &self,
        owner: &str,
        request: &model::page::ItemRequest,
    ) -> Result<Option<model::status::Status>> {
        let slot = request.slot()?;
        self.read(|s| {
            s.owned_source(owner, request.source)?
                .items
                .get(&slot)
                .map(|item| s.catalog.source_status(item.clone()))
                .transpose()
        })
    }
    pub fn plugin_source_items(&self, owner: &str, source: i64) -> Result<Vec<StoredItem>> {
        self.read(|s| {
            Ok(s.owned_source(owner, source)?
                .items
                .values()
                .cloned()
                .collect())
        })
    }
    pub fn plugin_bind_source(
        &self,
        expected: &StoredItem,
        revision: u64,
        resolved: &erika_catalog::resolve::Resolved,
    ) -> Result<()> {
        self.edit(|s| {
            if s.plugin_item(&expected.url).as_ref() != Some(expected) {
                return Err(Error::conflict("来源条目在读取资料期间已改变"));
            }
            s.apply(
                &resolved.nodes(),
                &[],
                &[erika_catalog::FileBinding {
                    file: expected.file,
                    revision,
                    decision: erika_catalog::Decision::Auto,
                    items: resolved.items.clone(),
                    evidence: resolved.evidence.clone(),
                }],
                &[],
            )?;
            Ok(())
        })
    }
}
