//! Structured catalogue operations use the same store transactions as the GUI.
//! Networking is asynchronous on both native hosts and the browser.
use super::{Done, LibraryEngine};
use bern::{CommandError, query::Answer};
use erika_catalog::{self as media, MediaId, MediaNode, resolve::{FileResolution, Resolution}};
use erika_library_protocol::library::{self, *};
use erika_store::catalog::{CatalogBatch, CatalogJob as Job};

#[cfg(test)]
mod tests;

pub(super) fn store_error(e: erika_store::catalog::Error) -> CommandError {
    CommandError::new(e.code, e.message)
}
fn error(message: &str) -> CommandError { CommandError::new("invalid", message) }
fn api_error(e: media::Error) -> CommandError { CommandError::new(format!("{:?}", e.code), e.message) }
fn revision(value: i64) -> Result<u64, CommandError> {
    value.try_into().map_err(|_| error("版本不能为负数"))
}
fn id(value: &str) -> Result<MediaId, CommandError> { MediaId::parse(value).map_err(api_error) }

macro_rules! enums {
    ($out:ident, $into:ident, $wire:ident, $model:ident, $($variant:ident),+) => {
        fn $out(value: media::$model) -> $wire {
            match value { $(media::$model::$variant => $wire::$variant),+ }
        }
        fn $into(value: &$wire) -> media::$model {
            match value { $($wire::$variant => media::$model::$variant),+ }
        }
    };
}
enums!(kind, input_kind, CatalogKind, Kind, Tv, Movie, Season, Episode, Subject, Item);
enums!(provider, input_provider, CatalogProvider, Provider, Tmdb, Dandanplay, Bangumi, Misaka, Jellyfin);
enums!(relation, input_relation, CatalogRelation, Relation, Exact, PartOf, Contains, Related);
enums!(decision, input_decision, CatalogDecision, Decision, Auto, Manual, Blocked);
fn reference(r: &media::ProviderRef) -> CatalogReference {
    CatalogReference { provider: provider(r.provider), scope: r.scope.clone(), kind: kind(r.kind), id: r.id.clone() }
}
fn input_reference(r: &CatalogReference) -> Result<media::ProviderRef, CommandError> {
    let r = media::ProviderRef { provider: input_provider(&r.provider), scope: r.scope.clone(), kind: input_kind(&r.kind), id: r.id.clone() };
    r.validate().map_err(api_error)?;
    Ok(r)
}
fn node(n: &MediaNode) -> Result<CatalogNode, CommandError> {
    Ok(CatalogNode {
        id: n.id.0.clone(), kind: kind(n.kind), root: n.root.0.clone(), parent: n.parent.as_ref().map(|id| id.0.clone()),
        season_number: n.season_number.map(i64::from), episode_number: n.episode_number.map(i64::from),
        reference: reference(n.reference.as_ref().ok_or_else(|| error("规范媒体缺少 TMDB 身份"))?),
        title: n.title.clone(), overview: n.overview.clone(), date: n.date.clone(), revision: n.revision as i64,
    })
}
fn item(i: &media::BindingItem) -> CatalogItem {
    CatalogItem { media: i.media.0.clone(), start_seconds: i.start_seconds, end_seconds: i.end_seconds }
}
fn binding(b: &media::FileBinding) -> CatalogBinding {
    CatalogBinding { file: b.file, revision: b.revision as i64, decision: decision(b.decision), items: b.items.iter().map(item).collect(), evidence: b.evidence.clone() }
}
fn danmaku(b: &media::DanmakuBinding) -> CatalogDanmaku {
    CatalogDanmaku { file: b.file, revision: b.revision as i64, reference: reference(&b.reference), media: b.media.as_ref().map(|i| i.0.clone()), manual: b.manual, fingerprint_verified: b.fingerprint_verified, shift_seconds: b.shift_seconds, user_shift_seconds: b.user_shift_seconds }
}
fn mapping(l: &media::ProviderLink) -> CatalogMapping {
    CatalogMapping { media: l.media.0.clone(), reference: reference(&l.reference), relation: relation(l.relation), coverage: l.coverage.iter().map(|i| i.0.clone()).collect(), evidence: l.evidence.clone(), confirmed: l.confirmed, manual: l.manual, revision: l.revision as i64 }
}
fn receipt(r: &erika_store::catalog::CatalogReceipt, preview: bool) -> CatalogReceipt {
    CatalogReceipt { request: r.request.clone(), change: if r.change.after.is_empty() && r.change.danmaku_after.is_empty() { String::new() } else { r.change.id.clone() }, preview, bindings: r.change.after.iter().map(binding).collect(), danmaku: r.change.danmaku_after.iter().map(danmaku).collect(), mappings: r.links.iter().map(mapping).collect() }
}
pub(super) fn input_bindings(values: Vec<CatalogBinding>) -> Result<Vec<media::FileBinding>, CommandError> {
    values.into_iter().map(|b| Ok(media::FileBinding {
        file: b.file, revision: revision(b.revision)?, decision: input_decision(&b.decision), evidence: b.evidence,
        items: b.items.into_iter().map(|i| Ok(media::BindingItem { media: id(&i.media)?, start_seconds: i.start_seconds, end_seconds: i.end_seconds })).collect::<Result<_, CommandError>>()?,
    })).collect()
}
pub(super) fn input_danmaku(values: Vec<CatalogDanmaku>) -> Result<Vec<media::DanmakuBinding>, CommandError> {
    values.into_iter().map(|b| Ok(media::DanmakuBinding {
        file: b.file, revision: revision(b.revision)?, reference: input_reference(&b.reference)?, media: b.media.as_deref().map(id).transpose()?, manual: b.manual, fingerprint_verified: b.fingerprint_verified, shift_seconds: b.shift_seconds, user_shift_seconds: b.user_shift_seconds,
    })).collect()
}
pub(super) fn input_mappings(values: Vec<CatalogMapping>) -> Result<Vec<media::ProviderLink>, CommandError> {
    values.into_iter().map(|l| Ok(media::ProviderLink {
        media: id(&l.media)?, reference: input_reference(&l.reference)?, relation: input_relation(&l.relation), coverage: l.coverage.iter().map(|i| id(i)).collect::<Result<_, _>>()?, evidence: l.evidence, confirmed: l.confirmed, manual: l.manual, revision: revision(l.revision)?,
    })).collect()
}

pub(super) enum Reply {
    Fetch { id: String, request: serde_json::Value, result: media::Result<Vec<MediaNode>> },
    Resolve { id: String, request: serde_json::Value, dry_run: bool, danmaku_revision: u64, report: FileResolution },
}
impl LibraryEngine {
    pub(super) fn api_apply(&mut self, request: String, batch: CatalogBatch, preview: bool) -> Result<(), CommandError> {
        let result = self.db.catalog_request(&request, &batch, preview).map_err(store_error)?;
        self.state.catalog_result = Some(receipt(&result, preview));
        if !preview { self.load(); library::emit::files_changed(); }
        Ok(())
    }
    pub(super) fn api_inspect(&self, file: i64) -> Result<CatalogInspection, CommandError> {
        self.db.catalog_addresses(file).map_err(store_error)?;
        let binding_value = self.db.catalog_binding(file).map_err(store_error)?;
        let dm = self.db.catalog_danmaku(file).map_err(store_error)?;
        let mut nodes = std::collections::BTreeMap::new();
        let mut pending: Vec<_> = binding_value.iter().flat_map(|b| b.items.iter().map(|i| i.media.clone())).chain(dm.iter().filter_map(|d| d.media.clone())).collect();
        while let Some(id) = pending.pop() {
            if nodes.contains_key(&id) { continue; }
            let n = self.db.catalog_node(&id).map_err(store_error)?.ok_or_else(|| error("媒体不存在"))?;
            if let Some(parent) = &n.parent { pending.push(parent.clone()); }
            nodes.insert(id, n);
        }
        let mut mappings = Vec::new();
        for id in nodes.keys() { mappings.extend(self.db.catalog_links(id).map_err(store_error)?.iter().map(mapping)); }
        Ok(CatalogInspection { file, revision: self.db.catalog_revision(file).map_err(store_error)? as i64, danmaku_revision: self.db.catalog_danmaku_revision(file).map_err(store_error)? as i64, binding: binding_value.as_ref().map(binding), danmaku: dm.as_ref().map(danmaku), nodes: nodes.values().map(node).collect::<Result<_, _>>()?, mappings })
    }
    pub(super) fn api_media(&self, value: &str) -> Result<CatalogNode, CommandError> {
        node(&self.db.catalog_node(&id(value)?).map_err(store_error)?.ok_or_else(|| error("媒体不存在"))?)
    }
    pub(super) fn api_children(&self, value: &str) -> Result<Vec<CatalogNode>, CommandError> {
        self.db.catalog_children(&id(value)?).map_err(store_error)?.iter().map(node).collect()
    }
    pub(super) fn api_receipt(&self, request: &str) -> Result<CatalogReceipt, CommandError> {
        Ok(receipt(&self.db.catalog_receipt(request).map_err(store_error)?.ok_or_else(|| error("请求尚未提交"))?, false))
    }
    pub(super) fn api_search(&self, kind: CatalogKind, text: String, year: Option<i64>, page: i64, answer: Answer<CatalogSearch>) {
        let parameters = (|| {
            let year = year.map(|y| u16::try_from(y).map_err(|_| error("年份不正确"))).transpose()?;
            let page = u32::try_from(page).map_err(|_| error("页码不正确"))?;
            Ok::<_, CommandError>((year, page))
        })();
        let (year, page) = match parameters { Ok(v) => v, Err(e) => { answer.send(Err(e)); return; } };
        let client = self.metadata.clone();
        media::client::spawn(async move {
            let result = match client.search(input_kind(&kind), &text, year, page).await {
                Ok(p) => p.results.iter().map(node).collect::<Result<_, _>>().map(|nodes| CatalogSearch { nodes, page: p.page as i64, total_pages: p.total_pages as i64 }),
                Err(e) => Err(api_error(e)),
            };
            answer.send(result);
        });
    }
    fn api_replayed(&self, request: &str, payload: &serde_json::Value) -> Result<bool, CommandError> {
        id(request)?;
        if let Some(job) = self.db.catalog_job(request).map_err(store_error)? {
            let old: serde_json::Value = serde_json::from_str(&job.payload).map_err(|_| error("任务记录损坏"))?;
            if old.get("request").or_else(|| old.get("input").and_then(|i| i.get("request"))) != Some(payload) { return Err(error("请求标识已用于另一项任务")); }
            return Ok(true);
        }
        Ok(false)
    }
    fn api_start_job(&self, request: &str, file: Option<i64>, rev: u64, phase: &str, payload: &serde_json::Value) -> Result<bool, CommandError> {
        if self.api_replayed(request, payload)? { return Ok(false); }
        self.db.catalog_job_enqueue(&Job { id: request.into(), file, input_revision: rev, phase: phase.into(), state: "queued".into(), payload: serde_json::json!({"request":payload}).to_string(), error: String::new(), next_attempt: 0 }).map_err(store_error)?;
        let job = self.db.catalog_job_transition(request, "queued", "running", phase, "", 0).map_err(store_error)?;
        if job.state != "running" { return Err(error("文件绑定已改变")); }
        Ok(true)
    }
    pub(super) fn api_fetch(&self, request: String, reference: CatalogReference, season: Option<i64>) -> Result<(), CommandError> {
        let reference = input_reference(&reference)?;
        if reference.provider != media::Provider::Tmdb || !reference.kind.work() { return Err(error("需要 TMDB 作品引用")); }
        let season = season.map(|s| u32::try_from(s).map_err(|_| error("季号不正确"))).transpose()?;
        if reference.kind == media::Kind::Movie && season.is_some() { return Err(error("电影没有季")); }
        let input = serde_json::json!({"operation":"fetch","reference":reference,"season":season});
        if !self.api_start_job(&request, None, 0, "fetch", &input)? { return Ok(()); }
        let client = self.metadata.clone();
        let send = sender(&self.inbox);
        media::client::spawn(async move {
            let result = async {
                let work = client.with_artwork(client.work(&reference).await?).await;
                let mut nodes = vec![work.node.clone()];
                nodes.extend(work.seasons.clone());
                if let Some(number) = season {
                    let season = client.season(&work, number).await?;
                    nodes.retain(|n| n.id != season.node.id);
                    nodes.push(season.node);
                    nodes.extend(season.episodes);
                }
                Ok(nodes)
            }.await;
            send(Reply::Fetch { id: request, request: input, result });
        });
        Ok(())
    }
    pub(super) fn api_resolve(&self, request: String, file: i64, rev: i64, dry_run: bool) -> Result<(), CommandError> {
        let rev = revision(rev)?;
        let input = serde_json::json!({"operation":"resolve","file":file,"revision":rev,"dry_run":dry_run});
        if self.api_replayed(&request, &input)? { return Ok(()); }
        let addresses = self.db.catalog_addresses(file).map_err(store_error)?;
        if self.db.catalog_revision(file).map_err(store_error)? != rev { return Err(CommandError::new("conflict", "文件绑定已改变")); }
        if let Some(binding) = self.db.catalog_binding(file).map_err(store_error)? {
            let mut nodes = std::collections::BTreeMap::new();
            let mut pending: Vec<_> = binding.items.iter().map(|i| i.media.clone()).collect();
            while let Some(id) = pending.pop() {
                if nodes.contains_key(&id) { continue; }
                let n = self.db.catalog_node(&id).map_err(store_error)?.ok_or_else(|| error("媒体不存在"))?;
                if let Some(parent) = &n.parent { pending.push(parent.clone()); }
                nodes.insert(id, n);
            }
            let reason = if binding.decision == media::Decision::Blocked { "已解除绑定；允许自动识别后才能重新解析" } else { "复用已有内容绑定" };
            let payload = serde_json::json!({"request":input,"nodes":nodes.into_values().collect::<Vec<_>>(),"items":binding.items,"reason":reason}).to_string();
            if self.api_start_job(&request, Some(file), rev, "reuse", &input)? {
                if let Err(e) = self.db.catalog_complete_read(&request, &payload, &[]) {
                    let _ = self.db.catalog_job_transition(&request, "running", "failed", "reuse", &e.message, 0);
                    return Err(store_error(e));
                }
            }
            return Ok(());
        }
        let entry = addresses.first().ok_or_else(|| error("文件没有可访问的地址"))?;
        let name = self.name_of(&entry.url, &[]);
        let directory = bern::files::file_path(&entry.url).and_then(|p| p.parent().map(|p| p.to_string_lossy().into_owned())).unwrap_or_default();
        let fp = self.db.fingerprint_of(file);
        let danmaku_revision = self.db.catalog_danmaku_revision(file).map_err(store_error)?;
        let kind = self.db.sources().iter().find(|s| s.id == entry.source).and_then(|s| match s.content.as_str() {
            "movie" => Some(media::Kind::Movie), "series" => Some(media::Kind::Tv), _ => None,
        });
        if !self.api_start_job(&request, Some(file), rev, "recognition", &input)? { return Ok(()); }
        let client = self.metadata.clone();
        let send = sender(&self.inbox);
        media::client::spawn(async move {
            let report = erika_dandanplay::catalog::resolve_file(&client, &name, &directory, fp.as_ref().map(|f| (f.md5.as_str(), f.size)), kind, None).await;
            send(Reply::Resolve { id: request, request: input, dry_run, danmaku_revision, report });
        });
        Ok(())
    }
    pub(super) fn take_api(&mut self, reply: Reply) -> bool {
        let (id, result, may_change) = match reply {
            Reply::Fetch { id, request, result } => {
                let result = match result {
                    Ok(nodes) => self.db.catalog_complete_read(&id, &serde_json::json!({"request":request,"nodes":nodes}).to_string(), &nodes),
                    Err(e) => self.db.catalog_job_transition(&id, "running", "failed", "fetch", &e.message, 0),
                };
                (id, result, true)
            }
            Reply::Resolve { id, request, dry_run, danmaku_revision, report } => {
                let result = if dry_run {
                    self.db.catalog_complete_read(&id, &serde_json::json!({"request":request,"report":report}).to_string(), &[])
                } else {
                    self.db.catalog_finish_resolution(&id, danmaku_revision, &report)
                };
                (id, result, !dry_run)
            }
        };
        match result {
            Ok(job) => may_change && !matches!(job.state.as_str(), "stale" | "failed" | "cancelled"),
            Err(e) => {
                let _ = self.db.catalog_job_transition(&id, "running", "failed", "complete", &e.message, 0);
                self.notify(LibraryNotice::Failed { reason: e.message });
                false
            }
        }
    }
    pub(super) fn api_job(&self, value: &str) -> Result<CatalogJob, CommandError> {
        id(value)?;
        let j = self.db.catalog_job(value).map_err(store_error)?.ok_or_else(|| error("任务不存在"))?;
        let payload: serde_json::Value = serde_json::from_str(&j.payload).map_err(|_| error("任务记录损坏"))?;
        let mut nodes: Vec<MediaNode> = payload.get("nodes").cloned().map(serde_json::from_value).transpose().map_err(|_| error("任务结果损坏"))?.unwrap_or_default();
        let mut items: Vec<media::BindingItem> = payload.get("items").cloned().map(serde_json::from_value).transpose().map_err(|_| error("任务结果损坏"))?.unwrap_or_default();
        let mut reason = payload.get("reason").and_then(|v| v.as_str()).unwrap_or_default().to_string();
        let report: Option<FileResolution> = payload.get("report").or_else(|| payload.get("result")).cloned().map(serde_json::from_value).transpose().map_err(|_| error("任务结果损坏"))?;
        if let Some(report) = report {
            match report.resolution {
                Ok(Resolution::Bound { result }) => { nodes = result.nodes(); items = result.items; reason = result.evidence; }
                Ok(Resolution::Pending { candidates, reason: why, .. }) => { nodes = candidates; reason = why; }
                Err(e) => reason = e.message,
            }
        }
        let mut stored = std::collections::HashMap::new();
        for n in &nodes {
            if let Some(reference) = &n.reference {
                if let Some(local) = self.db.catalog_by_ref(reference).map_err(store_error)?.into_iter().next() {
                    stored.insert(n.id.clone(), local);
                }
            }
        }
        let resolve = |id: &MediaId| stored.get(id).map(|n| n.id.clone()).unwrap_or_else(|| id.clone());
        nodes = nodes.into_iter().map(|mut n| {
            if let Some(local) = stored.get(&n.id) { return local.clone(); }
            n.root = resolve(&n.root);
            n.parent = n.parent.as_ref().map(&resolve);
            n
        }).collect();
        for item in &mut items { item.media = resolve(&item.media); }
        Ok(CatalogJob { id: j.id, file: j.file, input_revision: j.input_revision as i64, state: j.state, phase: j.phase, error: j.error, next_attempt: j.next_attempt, nodes: nodes.iter().map(node).collect::<Result<_, _>>()?, items: items.iter().map(item).collect(), reason })
    }
}
fn sender(inbox: &std::sync::Arc<std::sync::Mutex<Vec<Done>>>) -> impl Fn(Reply) + Send + 'static {
    let inbox = std::sync::Arc::clone(inbox);
    let waker = bern::wake::waker();
    move |reply| {
        inbox.lock().unwrap_or_else(|p| p.into_inner()).push(Done::Catalog(reply));
        if let Some(waker) = &waker { waker.wake(); }
    }
}
