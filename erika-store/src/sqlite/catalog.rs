//! Canonical catalogue operations. Every compound write owns an IMMEDIATE
//! transaction; validation errors and stale revisions roll back the whole edit.
use super::Db;
use crate::catalog::{
    BindingChange, Error, MediaFile, MediaMark, MediaShelf, MediaWatching, Result,
};
use erika_catalog::{
    BindingItem, DanmakuBinding, Decision, FileBinding, Kind, MediaId, MediaNode, Provider,
    ProviderLink, ProviderRef, Relation, Watched,
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Serialize, de::DeserializeOwned};
use std::collections::HashSet;

pub(super) fn json<T: Serialize>(value: &T) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}
pub(super) fn decode<T: DeserializeOwned>(text: String) -> Result<T> {
    Ok(serde_json::from_str(&text)?)
}
fn word<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_value(value)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| Error::invalid("枚举格式不正确"))
}
pub(super) fn transaction(conn: &Connection) -> Result<Transaction<'_>> {
    Ok(Transaction::new_unchecked(
        conn,
        TransactionBehavior::Immediate,
    )?)
}
pub(super) fn canonical(_conn: &Connection, id: &MediaId) -> Result<MediaId> {
    Ok(MediaId::parse(&id.0)?)
}
pub(super) fn node(conn: &Connection, id: &MediaId) -> Result<Option<MediaNode>> {
    let id = canonical(conn, id)?;
    conn.query_row("SELECT data FROM media_node WHERE id=?1", [&id.0], |r| {
        r.get::<_, String>(0)
    })
    .optional()?
    .map(decode)
    .transpose()
}
fn require_node(conn: &Connection, id: &MediaId) -> Result<MediaNode> {
    node(conn, id)?.ok_or_else(|| Error::invalid("媒体尚未入库"))
}
pub(super) fn sql_revision(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| Error::invalid("记录版本超出范围"))
}
fn next_revision(value: u64) -> Result<u64> {
    let next = value
        .checked_add(1)
        .ok_or_else(|| Error::conflict("记录版本超出范围"))?;
    sql_revision(next)?;
    Ok(next)
}
fn read_revision(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
}
pub(super) fn revision(conn: &Connection, file: i64) -> Result<u64> {
    Ok(conn
        .query_row(
            "SELECT revision FROM file_revision WHERE file_id=?1",
            [file],
            |r| read_revision(r, 0),
        )
        .optional()?
        .unwrap_or(0))
}
pub(super) fn binding(conn: &Connection, file: i64) -> Result<Option<FileBinding>> {
    let row = conn
        .query_row(
            "SELECT decision,evidence,revision FROM file_binding WHERE file_id=?1",
            [file],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    read_revision(r, 2)?,
                ))
            },
        )
        .optional()?;
    let Some((decision, evidence, revision)) = row else {
        return Ok(None);
    };
    let mut stmt = conn.prepare("SELECT media_id,start_seconds,end_seconds FROM file_binding_item WHERE file_id=?1 ORDER BY ordinal")?;
    let items = stmt
        .query_map([file], |r| {
            Ok(BindingItem {
                media: MediaId(r.get(0)?),
                start_seconds: r.get(1)?,
                end_seconds: r.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Some(FileBinding {
        file,
        decision: serde_json::from_value(serde_json::Value::String(decision))?,
        items,
        evidence,
        revision,
    }))
}
pub(super) fn bump(conn: &Connection, file: i64, previous: u64) -> Result<u64> {
    let next = previous
        .checked_add(1)
        .filter(|v| *v <= i64::MAX as u64)
        .ok_or_else(|| Error::conflict("绑定版本已超出范围"))?;
    conn.execute("INSERT INTO file_revision(file_id,revision) VALUES(?1,?2) ON CONFLICT(file_id) DO UPDATE SET revision=excluded.revision", params![file,sql_revision(next)?])?;
    Ok(next)
}
pub(super) fn put_node(conn: &Connection, input: &MediaNode) -> Result<MediaNode> {
    input.validate()?;
    let mut value = input.clone();
    if let Some(parent) = &value.parent {
        let parent = require_node(conn, parent)?;
        let allowed = matches!(
            (value.kind, parent.kind),
            (Kind::Season, Kind::Tv) | (Kind::Episode, Kind::Season)
        );
        if !allowed
            || parent.root != value.root
            || (value.kind == Kind::Episode && value.season_number != parent.season_number)
        {
            return Err(Error::invalid("媒体父级、季号或所属作品不一致"));
        }
    }
    let previous = node(conn, &value.id)?;
    if let Some(previous) = &previous {
        if previous.kind != value.kind
            || previous.parent != value.parent
            || previous.root != value.root
            || previous.reference != value.reference
            || previous.season_number != value.season_number
            || previous.episode_number != value.episode_number
        {
            return Err(Error::conflict("媒体身份或层级不能由资料刷新改写"));
        }
        if value.revision != 0 && value.revision != previous.revision {
            return Err(Error::conflict("媒体资料版本已改变"));
        }
        value.retain_metadata(previous);
    }
    value.validate_artwork()?;
    value.revision = next_revision(previous.as_ref().map_or(0, |n| n.revision))?;
    let tmdb_kind = value.reference.as_ref().map(|r| r.kind.as_str());
    let tmdb_id = value.reference.as_ref().map(|r| r.id.as_str());
    conn.execute("INSERT INTO media_node(id,kind,parent_id,root_id,season_number,episode_number,tmdb_kind,tmdb_id,title,data,revision) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11) ON CONFLICT(id) DO UPDATE SET title=excluded.title,data=excluded.data,revision=excluded.revision", params![value.id.0,value.kind.as_str(),value.parent.as_ref().map(|p| &p.0),value.root.0,value.season_number,value.episode_number,tmdb_kind,tmdb_id,value.title,json(&value)?,sql_revision(value.revision)?])?;
    for asset in [
        &value.artwork.poster,
        &value.artwork.backdrop,
        &value.artwork.logo,
        &value.artwork.still,
    ]
    .into_iter()
    .flatten()
    {
        conn.execute("INSERT INTO media_artwork(media_id,kind,path,data) VALUES(?1,?2,?3,?4) ON CONFLICT(media_id,kind,path) DO UPDATE SET data=excluded.data", params![value.id.0,word(&asset.kind)?,asset.file_path,json(asset)?])?;
    }
    Ok(value)
}
pub(super) fn identities(
    conn: &Connection,
    nodes: &[MediaNode],
) -> Result<crate::catalog_identity::Identities> {
    crate::catalog_identity::Identities::prepare(
        nodes,
        |id| node(conn, id),
        |r| {
            conn.query_row(
                "SELECT data FROM media_node WHERE tmdb_kind=?1 AND tmdb_id=?2",
                params![r.kind.as_str(), r.id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(decode)
            .transpose()
        },
    )
}
pub(super) fn save_nodes(conn: &Connection, nodes: &[MediaNode]) -> Result<Vec<MediaNode>> {
    let ids = identities(conn, nodes)?;
    let nodes: Vec<_> = nodes.iter().map(|n| ids.node(n)).collect();
    put_nodes(conn, &nodes)?;
    nodes.iter().map(|n| require_node(conn, &n.id)).collect()
}
pub(super) fn put_nodes(conn: &Connection, nodes: &[MediaNode]) -> Result<()> {
    if nodes.len() > 100_000 {
        return Err(Error::invalid("媒体记录过多"));
    }
    let mut seen = HashSet::new();
    let mut sorted: Vec<_> = nodes.iter().collect();
    sorted.sort_by_key(|n| match n.kind {
        Kind::Tv | Kind::Movie => 0,
        Kind::Season => 1,
        _ => 2,
    });
    for n in sorted {
        if !seen.insert(&n.id) {
            return Err(Error::invalid("媒体记录重复"));
        }
        put_node(conn, n)?;
    }
    Ok(())
}
pub(super) fn put_link(conn: &Connection, input: &ProviderLink) -> Result<ProviderLink> {
    input.reference.validate()?;
    let media = require_node(conn, &input.media)?;
    if input.evidence.len() > 65536 {
        return Err(Error::invalid("关联证据过长"));
    }
    if input.reference.provider == Provider::Tmdb
        && (input.relation != Relation::Exact
            || media.reference.as_ref() != Some(&input.reference)
            || !input.coverage.is_empty())
    {
        return Err(Error::invalid("TMDB 关联必须是该节点的精确身份"));
    }
    let mut coverage = HashSet::new();
    for id in &input.coverage {
        let covered = require_node(conn, id)?;
        if !covered.kind.playable()
            || covered.root != media.root
            || !coverage.insert(id)
            || (media.kind == Kind::Season && covered.parent.as_ref() != Some(&media.id))
            || (media.kind.playable() && covered.id != media.id)
        {
            return Err(Error::invalid("关联覆盖范围不属于当前媒体"));
        }
    }
    let r = &input.reference;
    let previous: Option<ProviderLink> = conn.query_row("SELECT data FROM provider_link WHERE media_id=?1 AND provider=?2 AND scope=?3 AND kind=?4 AND external_id=?5", params![media.id.0,word(&r.provider)?,r.scope,r.kind.as_str(),r.id], |row| row.get::<_, String>(0)).optional()?.map(decode).transpose()?;
    if previous.as_ref().map_or(0, |l| l.revision) != input.revision {
        return Err(Error::conflict("外部关联版本已改变"));
    }
    if previous.as_ref().is_some_and(|l| l.manual) && !input.manual {
        return Err(Error::conflict("自动关联不能覆盖人工关联"));
    }
    let mut value = input.clone();
    value.revision = next_revision(value.revision)?;
    conn.execute("INSERT INTO provider_link(media_id,provider,scope,kind,external_id,relation,confirmed,data,revision) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(media_id,provider,scope,kind,external_id) DO UPDATE SET relation=excluded.relation,confirmed=excluded.confirmed,data=excluded.data,revision=excluded.revision", params![media.id.0,word(&r.provider)?,r.scope,r.kind.as_str(),r.id,word(&value.relation)?,value.confirmed,json(&value)?,sql_revision(value.revision)?])?;
    Ok(value)
}
pub(super) fn put_binding(conn: &Connection, input: &FileBinding) -> Result<FileBinding> {
    input.validate()?;
    if input.decision != Decision::Blocked && input.items.is_empty() {
        return Err(Error::invalid("绑定必须包含可播放媒体"));
    }
    if input.evidence.len() > 65536 {
        return Err(Error::invalid("绑定证据过长"));
    }
    let rev = revision(conn, input.file)?;
    if rev != input.revision {
        return Err(Error::conflict("文件绑定已改变，丢弃过期结果"));
    }
    let previous = binding(conn, input.file)?;
    if input.decision == Decision::Auto
        && previous
            .as_ref()
            .is_some_and(|b| b.decision != Decision::Auto)
    {
        return Err(Error::conflict("自动识别不能覆盖人工决定"));
    }
    let mut roots = HashSet::new();
    for item in &input.items {
        let media = require_node(conn, &item.media)?;
        if media.id != item.media || !media.kind.playable() {
            return Err(Error::invalid("文件只能绑定当前可播放媒体"));
        }
        roots.insert(media.root);
    }
    if roots.len() > 1 {
        return Err(Error::invalid("一个文件的分集必须属于同一作品"));
    }
    let mut value = input.clone();
    value.revision = bump(conn, input.file, rev)?;
    conn.execute("INSERT INTO file_binding(file_id,decision,evidence,revision) VALUES(?1,?2,?3,?4) ON CONFLICT(file_id) DO UPDATE SET decision=excluded.decision,evidence=excluded.evidence,revision=excluded.revision", params![value.file,word(&value.decision)?,value.evidence,sql_revision(value.revision)?])?;
    conn.execute(
        "DELETE FROM file_binding_item WHERE file_id=?1",
        [value.file],
    )?;
    for (index, item) in value.items.iter().enumerate() {
        conn.execute("INSERT INTO file_binding_item(file_id,media_id,ordinal,start_seconds,end_seconds) VALUES(?1,?2,?3,?4,?5)", params![value.file,item.media.0,index as i64,item.start_seconds,item.end_seconds])?;
    }
    // Never retain a legacy content identity after a canonical rematch. Danmaku
    // has its own table and is deliberately not overwritten by this operation.
    conn.execute(
        "UPDATE file SET episode_id=NULL, matched_by=?2 WHERE id=?1",
        params![
            value.file,
            match value.decision {
                Decision::Auto => "auto",
                Decision::Manual => "manual",
                Decision::Blocked => "none",
            }
        ],
    )?;
    Ok(value)
}
pub(super) fn danmaku_revision(conn: &Connection, file: i64) -> Result<u64> {
    Ok(conn
        .query_row(
            "SELECT revision FROM danmaku_revision WHERE file_id=?1",
            [file],
            |r| read_revision(r, 0),
        )
        .optional()?
        .unwrap_or(0))
}
fn bump_danmaku(conn: &Connection, file: i64, previous: u64) -> Result<u64> {
    let next = next_revision(previous)?;
    conn.execute("INSERT INTO danmaku_revision(file_id,revision) VALUES(?1,?2) ON CONFLICT(file_id) DO UPDATE SET revision=excluded.revision",params![file,sql_revision(next)?])?;
    Ok(next)
}
pub(super) fn put_danmaku(conn: &Connection, input: &DanmakuBinding) -> Result<DanmakuBinding> {
    input.reference.validate()?;
    if input.reference.kind != Kind::Episode
        || !matches!(
            input.reference.provider,
            Provider::Dandanplay | Provider::Misaka
        )
        || !input.shift_seconds.is_finite()
        || !input.user_shift_seconds.is_finite()
        || !(input.shift_seconds + input.user_shift_seconds).is_finite()
    {
        return Err(Error::invalid("弹幕关联或偏移不正确"));
    }
    if let Some(media) = &input.media {
        if !require_node(conn, media)?.kind.playable() {
            return Err(Error::invalid("弹幕关联的媒体不可播放"));
        }
    }
    let previous: Option<DanmakuBinding> = conn
        .query_row(
            "SELECT data FROM danmaku_binding WHERE file_id=?1",
            [input.file],
            |r| r.get::<_, String>(0),
        )
        .optional()?
        .map(decode)
        .transpose()?;
    if danmaku_revision(conn, input.file)? != input.revision {
        return Err(Error::conflict("弹幕关联已改变"));
    }
    if previous.as_ref().is_some_and(|b| b.manual) && !input.manual {
        return Err(Error::conflict("自动匹配不能覆盖人工弹幕关联"));
    }
    let mut value = input.clone();
    value.revision = bump_danmaku(conn, value.file, value.revision)?;
    conn.execute("INSERT INTO danmaku_binding(file_id,data,revision) VALUES(?1,?2,?3) ON CONFLICT(file_id) DO UPDATE SET data=excluded.data,revision=excluded.revision", params![value.file,json(&value)?,sql_revision(value.revision)?])?;
    Ok(value)
}
pub(super) fn put_watched(conn: &Connection, input: &Watched) -> Result<Watched> {
    let media = require_node(conn, &input.media)?;
    if media.id != input.media || !media.kind.playable() {
        return Err(Error::invalid("只能标记可播放媒体的观看状态"));
    }
    let previous: Option<Watched> = conn
        .query_row(
            "SELECT watched,manual,updated_at,revision FROM media_watched WHERE media_id=?1",
            [&input.media.0],
            |r| {
                Ok(Watched {
                    media: input.media.clone(),
                    watched: r.get(0)?,
                    manual: r.get(1)?,
                    updated_at: r.get(2)?,
                    revision: read_revision(r, 3)?,
                })
            },
        )
        .optional()?;
    if previous.as_ref().map_or(0, |w| w.revision) != input.revision {
        return Err(Error::conflict("观看记录已改变"));
    }
    if previous.as_ref().is_some_and(|w| w.manual) && !input.manual {
        return Err(Error::conflict("自动进度不能覆盖人工观看状态"));
    }
    let mut value = input.clone();
    value.revision = next_revision(value.revision)?;
    conn.execute("INSERT INTO media_watched(media_id,watched,manual,updated_at,revision) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(media_id) DO UPDATE SET watched=excluded.watched,manual=excluded.manual,updated_at=excluded.updated_at,revision=excluded.revision", params![value.media.0,value.watched,value.manual,value.updated_at,sql_revision(value.revision)?])?;
    Ok(value)
}
pub(super) fn mark(
    conn: &Connection,
    media: &MediaId,
    reference: &Option<ProviderRef>,
) -> Result<Option<MediaMark>> {
    conn.query_row(
        "SELECT data FROM media_mark WHERE media_id=?1 AND reference=?2",
        params![media.0, json(reference)?],
        |r| r.get::<_, String>(0),
    )
    .optional()?
    .map(decode)
    .transpose()
}
pub(super) fn put_mark(conn: &Connection, input: &MediaMark) -> Result<MediaMark> {
    let media = require_node(conn, &input.media)?;
    let previous = mark(conn, &input.media, &input.reference)?;
    let value = input.stored(&media, previous.as_ref())?;
    conn.execute("INSERT INTO media_mark(media_id,reference,data) VALUES(?1,?2,?3) ON CONFLICT(media_id,reference) DO UPDATE SET data=excluded.data", params![value.media.0,json(&value.reference)?,json(&value)?])?;
    Ok(value)
}

/// Called only inside the caller's transaction, including a job's final state.
pub(super) fn apply(
    conn: &Connection,
    nodes: &[MediaNode],
    links: &[ProviderLink],
    bindings: &[FileBinding],
    danmaku: &[DanmakuBinding],
) -> Result<BindingChange> {
    if bindings.len() > 10000 || danmaku.len() > 10000 {
        return Err(Error::invalid("批量修改过大"));
    }
    let mut unique = HashSet::new();
    let mut before = Vec::new();
    let mut after = Vec::new();
    for b in bindings {
        if !unique.insert(b.file) {
            return Err(Error::invalid("批量绑定包含重复文件"));
        }
        before.push(binding(conn, b.file)?);
    }
    let ids = identities(conn, nodes)?;
    put_nodes(conn, &nodes.iter().map(|n| ids.node(n)).collect::<Vec<_>>())?;
    for link in links {
        put_link(conn, &ids.link(link))?;
    }
    for b in bindings {
        after.push(put_binding(conn, &ids.binding(b))?);
    }
    unique.clear();
    let mut danmaku_before = Vec::new();
    let mut danmaku_after = Vec::new();
    for b in danmaku {
        if !unique.insert(b.file) {
            return Err(Error::invalid("批量弹幕关联包含重复文件"));
        }
        danmaku_before.push(
            conn.query_row(
                "SELECT data FROM danmaku_binding WHERE file_id=?1",
                [b.file],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .map(decode)
            .transpose()?,
        );
        danmaku_after.push(put_danmaku(conn, &ids.danmaku(b))?);
    }
    let change = BindingChange {
        id: MediaId::new().0,
        before,
        after,
        danmaku_before,
        danmaku_after,
    };
    if !change.after.is_empty() || !change.danmaku_after.is_empty() {
        conn.execute(
            "INSERT INTO catalog_change(id,data,created_at) VALUES(?1,?2,?3)",
            params![change.id, json(&change)?, crate::now()],
        )?;
    }
    Ok(change)
}

impl Db {
    /// Related identities and revision clocks must come from the same snapshot.
    pub fn catalog_context(&self, file: i64) -> Result<crate::catalog_context::MediaContext> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Deferred)?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM file WHERE id=?1)",
            [file],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(Error::invalid("文件不存在"));
        }
        let result = crate::catalog_context::MediaContext::new(
            file,
            self.catalog_revision(file)?,
            self.catalog_danmaku_revision(file)?,
            self.catalog_binding(file)?,
            self.catalog_danmaku(file)?,
        )
        .hydrate(|id| Ok((require_node(&tx, id)?, self.catalog_links(id)?)))?;
        tx.commit()?;
        Ok(result)
    }

    pub fn catalog_node(&self, id: &MediaId) -> Result<Option<MediaNode>> {
        node(&self.conn, id)
    }
    /// Compare only revisions; metadata caches need not deserialize every node
    /// on each tick. A missing row invalidates the captured cache too.
    pub fn catalog_nodes_unchanged(&self, expected: &[(MediaId, u64)]) -> Result<bool> {
        if expected.len() > 10000 {
            return Err(Error::invalid("媒体记录过多"));
        }
        Ok(self.conn.query_row(
            "SELECT NOT EXISTS(SELECT 1 FROM json_each(?1) e LEFT JOIN media_node n ON n.id=json_extract(e.value,'$[0]') WHERE n.revision IS NULL OR n.revision!=json_extract(e.value,'$[1]'))",
            [json(&expected)?], |r| r.get(0),
        )?)
    }
    pub fn catalog_revision(&self, file: i64) -> Result<u64> {
        revision(&self.conn, file)
    }
    pub fn catalog_binding(&self, file: i64) -> Result<Option<FileBinding>> {
        binding(&self.conn, file)
    }
    /// Return the actual local identities and revisions, in input order.
    pub fn catalog_save_nodes(&self, nodes: &[MediaNode]) -> Result<Vec<MediaNode>> {
        let tx = transaction(&self.conn)?;
        let stored = save_nodes(&tx, nodes)?;
        tx.commit()?;
        Ok(stored)
    }
    pub fn catalog_link(&self, link: &ProviderLink) -> Result<ProviderLink> {
        let tx = transaction(&self.conn)?;
        let result = put_link(&tx, link)?;
        tx.commit()?;
        Ok(result)
    }
    pub fn catalog_links(&self, id: &MediaId) -> Result<Vec<ProviderLink>> {
        let id = canonical(&self.conn, id)?;
        let mut stmt = self.conn.prepare("SELECT data FROM provider_link WHERE media_id=?1 ORDER BY provider,scope,kind,external_id")?;
        stmt.query_map([&id.0], |r| r.get::<_, String>(0))?
            .map(|r| decode(r?))
            .collect()
    }
    pub fn catalog_by_ref(&self, reference: &ProviderRef) -> Result<Vec<MediaNode>> {
        reference.validate()?;
        let mut ids: Vec<MediaId> = Vec::new();
        if reference.provider == Provider::Tmdb {
            if let Some(id) = self
                .conn
                .query_row(
                    "SELECT id FROM media_node WHERE tmdb_kind=?1 AND tmdb_id=?2",
                    params![reference.kind.as_str(), reference.id],
                    |r| r.get::<_, String>(0),
                )
                .optional()?
            {
                ids.push(MediaId(id));
            }
        }
        let mut stmt = self.conn.prepare("SELECT media_id FROM provider_link WHERE provider=?1 AND scope=?2 AND kind=?3 AND external_id=?4 AND confirmed=1 AND relation='exact'")?;
        for id in stmt.query_map(
            params![
                word(&reference.provider)?,
                reference.scope,
                reference.kind.as_str(),
                reference.id
            ],
            |r| r.get::<_, String>(0),
        )? {
            let id = canonical(&self.conn, &MediaId(id?))?;
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        ids.iter().map(|id| require_node(&self.conn, id)).collect()
    }
    pub fn catalog_children(&self, id: &MediaId) -> Result<Vec<MediaNode>> {
        let id = canonical(&self.conn, id)?;
        let mut stmt = self.conn.prepare("SELECT data FROM media_node WHERE parent_id=?1 ORDER BY season_number,episode_number,id")?;
        stmt.query_map([&id.0], |r| r.get::<_, String>(0))?
            .map(|r| decode(r?))
            .collect()
    }
    /// Input revisions are the values read BEFORE network work or an edit began.
    /// No part of a rejected operation (including new metadata) is committed.
    pub fn catalog_apply(
        &self,
        nodes: &[MediaNode],
        links: &[ProviderLink],
        bindings: &[FileBinding],
        danmaku: &[DanmakuBinding],
    ) -> Result<BindingChange> {
        self.catalog_apply_from_job(nodes, links, bindings, danmaku, None)
    }
    pub fn catalog_apply_from_job(
        &self,
        nodes: &[MediaNode],
        links: &[ProviderLink],
        bindings: &[FileBinding],
        danmaku: &[DanmakuBinding],
        job: Option<&str>,
    ) -> Result<BindingChange> {
        if bindings.len() > 10000 || danmaku.len() > 10000 {
            return Err(Error::invalid("批量修改过大"));
        }
        let tx = transaction(&self.conn)?;
        let job: Option<crate::catalog::CatalogJob> = job
            .map(|id| -> Result<_> {
                let job: crate::catalog::CatalogJob = tx
                    .query_row(
                        "SELECT data FROM catalog_job WHERE id=?1 AND state='running'",
                        [id],
                        |r| r.get::<_, String>(0),
                    )
                    .optional()?
                    .map(decode)
                    .transpose()?
                    .ok_or_else(|| Error::conflict("识别任务已失效"))?;
                if bindings.len() != 1
                    || Some(bindings[0].file) != job.file
                    || bindings[0].revision != job.input_revision
                {
                    return Err(Error::conflict("识别结果不属于此任务或文件版本"));
                }
                Ok(job)
            })
            .transpose()?;
        let change = apply(&tx, nodes, links, bindings, danmaku)?;
        if let Some(mut job) = job {
            job.state = "bound".into();
            job.phase = "complete".into();
            job.error.clear();
            tx.execute(
                "UPDATE catalog_job SET state='bound',data=?2 WHERE id=?1",
                params![job.id, json(&job)?],
            )?;
        }
        tx.commit()?;
        Ok(change)
    }
    pub fn catalog_undo(&self, change_id: &str) -> Result<()> {
        let tx = transaction(&self.conn)?;
        let change: BindingChange = tx
            .query_row(
                "SELECT data FROM catalog_change WHERE id=?1 AND undone=0",
                [change_id],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .map(decode)
            .transpose()?
            .ok_or_else(|| Error::conflict("修改不存在或已撤销"))?;
        for after in &change.after {
            if binding(&tx, after.file)?.as_ref() != Some(after) {
                return Err(Error::conflict("文件在此后又有修改，不能覆盖撤销"));
            }
        }
        for (before, after) in change.before.iter().zip(&change.after) {
            match before {
                Some(before) => {
                    let mut before = before.clone();
                    before.revision = after.revision;
                    // Restore an old auto decision even when the edit was manual.
                    tx.execute("DELETE FROM file_binding WHERE file_id=?1", [after.file])?;
                    put_binding(&tx, &before)?;
                }
                None => {
                    tx.execute("DELETE FROM file_binding WHERE file_id=?1", [after.file])?;
                    bump(&tx, after.file, after.revision)?;
                    tx.execute(
                        "UPDATE file SET episode_id=NULL,matched_by=NULL WHERE id=?1",
                        [after.file],
                    )?;
                }
            }
        }
        for after in &change.danmaku_after {
            if self.catalog_danmaku(after.file)?.as_ref() != Some(after) {
                return Err(Error::conflict("弹幕关联在此后又有修改，不能覆盖撤销"));
            }
        }
        for (before, after) in change.danmaku_before.iter().zip(&change.danmaku_after) {
            tx.execute("DELETE FROM danmaku_binding WHERE file_id=?1", [after.file])?;
            match before {
                Some(before) => {
                    let mut before = before.clone();
                    before.revision = after.revision;
                    put_danmaku(&tx, &before)?;
                }
                None => {
                    bump_danmaku(&tx, after.file, after.revision)?;
                }
            }
        }
        tx.execute(
            "UPDATE catalog_change SET undone=1 WHERE id=?1",
            [change_id],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn catalog_danmaku_revision(&self, file: i64) -> Result<u64> {
        danmaku_revision(&self.conn, file)
    }
    pub fn catalog_danmaku(&self, file: i64) -> Result<Option<DanmakuBinding>> {
        self.conn
            .query_row(
                "SELECT data FROM danmaku_binding WHERE file_id=?1",
                [file],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .map(decode)
            .transpose()
    }
    pub fn catalog_set_danmaku(&self, value: &DanmakuBinding) -> Result<DanmakuBinding> {
        let tx = transaction(&self.conn)?;
        let result = put_danmaku(&tx, value)?;
        tx.commit()?;
        Ok(result)
    }
    pub fn catalog_set_watched(&self, value: &Watched) -> Result<Watched> {
        let tx = transaction(&self.conn)?;
        let result = put_watched(&tx, value)?;
        tx.commit()?;
        Ok(result)
    }
    /// Commit a mark only while the captured provider associations remain exact,
    /// unique and unchanged. These reads share the write transaction/connection.
    pub fn catalog_set_watched_linked(
        &self,
        value: &Watched,
        links: &[ProviderLink],
    ) -> Result<Watched> {
        let tx = transaction(&self.conn)?;
        crate::catalog::check_watched_links(value, links, |link| {
            Ok((
                self.catalog_links(&link.media)?,
                self.catalog_by_ref(&link.reference)?
                    .into_iter()
                    .map(|node| node.id)
                    .collect(),
            ))
        })?;
        let result = put_watched(&tx, value)?;
        tx.commit()?;
        Ok(result)
    }
    pub fn catalog_watched(&self, id: &MediaId) -> Result<Option<Watched>> {
        let id = canonical(&self.conn, id)?;
        Ok(self
            .conn
            .query_row(
                "SELECT watched,manual,updated_at,revision FROM media_watched WHERE media_id=?1",
                [&id.0],
                |r| {
                    Ok(Watched {
                        media: id.clone(),
                        watched: r.get(0)?,
                        manual: r.get(1)?,
                        updated_at: r.get(2)?,
                        revision: read_revision(r, 3)?,
                    })
                },
            )
            .optional()?)
    }
    pub fn catalog_set_mark(&self, value: &MediaMark) -> Result<MediaMark> {
        let tx = transaction(&self.conn)?;
        let stored = put_mark(&tx, value)?;
        tx.commit()?;
        Ok(stored)
    }
    pub fn catalog_edit_mark(
        &self,
        media: &MediaId,
        edit: &crate::catalog::MarkEdit,
    ) -> Result<MediaMark> {
        let tx = transaction(&self.conn)?;
        let mut value = mark(&tx, media, &None)?.unwrap_or_else(|| MediaMark::local(media.clone()));
        value.edit(edit, crate::now());
        let stored = put_mark(&tx, &value)?;
        tx.commit()?;
        Ok(stored)
    }
    pub fn catalog_marks(&self, id: &MediaId) -> Result<Vec<MediaMark>> {
        let id = canonical(&self.conn, id)?;
        let mut stmt = self
            .conn
            .prepare("SELECT data FROM media_mark WHERE media_id=?1 ORDER BY reference")?;
        stmt.query_map([&id.0], |r| r.get::<_, String>(0))?
            .map(|r| decode(r?))
            .collect()
    }
    /// One address per file, not one file per episode: alternate encodes stay.
    pub fn catalog_files(&self, id: &MediaId) -> Result<Vec<MediaFile>> {
        let id = canonical(&self.conn, id)?;
        let mut stmt = self.conn.prepare("SELECT bi.file_id,bi.media_id,ad.path,coalesce(p.position,0),coalesce(p.duration,0) FROM file_binding_item bi JOIN media_node n ON n.id=bi.media_id JOIN address ad ON ad.file_id=bi.file_id LEFT JOIN progress p ON p.file_id=bi.file_id WHERE n.id=?1 OR n.root_id=?1 OR n.parent_id=?1 ORDER BY n.season_number,n.episode_number,bi.file_id,ad.seen_at DESC,ad.path")?;
        let rows = stmt
            .query_map([&id.0], |r| {
                Ok(MediaFile {
                    file: r.get(0)?,
                    media: MediaId(r.get(1)?),
                    url: r.get(2)?,
                    position: r.get(3)?,
                    duration: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut seen = HashSet::new();
        Ok(rows
            .into_iter()
            .filter(|f| seen.insert((f.file, f.media.clone())))
            .collect())
    }
    pub fn catalog_shelf(&self, source: Option<i64>) -> Result<Vec<MediaShelf>> {
        let mut stmt = self.conn.prepare("SELECT root.data,count(DISTINCT n.id),count(DISTINCT CASE WHEN w.watched=1 THEN n.id END),min(ad.seen_at),max(ad.seen_at) FROM media_node root JOIN media_node n ON n.root_id=root.id JOIN file_binding_item bi ON bi.media_id=n.id JOIN address ad ON ad.file_id=bi.file_id LEFT JOIN media_watched w ON w.media_id=n.id WHERE root.parent_id IS NULL AND (?1 IS NULL OR ad.source_id=?1) GROUP BY root.id ORDER BY max(ad.seen_at) DESC,root.id")?;
        stmt.query_map([source], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
            ))
        })?
        .map(|row| {
            let (data, episodes, watched, added, seen) = row?;
            Ok(MediaShelf {
                node: decode(data)?,
                episodes,
                watched,
                added,
                seen,
            })
        })
        .collect()
    }
    pub fn catalog_watching(&self, limit: usize) -> Result<Vec<MediaWatching>> {
        let mut stmt = self.conn.prepare("SELECT ad.file_id,r.path,r.still,coalesce(p.position,r.position),coalesce(p.duration,r.duration) FROM recent r JOIN address ad ON ad.source_id=r.source_id AND ad.path=r.path JOIN file_binding b ON b.file_id=ad.file_id LEFT JOIN progress p ON p.file_id=ad.file_id ORDER BY r.played_at DESC,r.path")?;
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for row in stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, f64>(3)?,
                r.get::<_, f64>(4)?,
            ))
        })? {
            if result.len() >= limit {
                break;
            }
            let (file, url, still, position, duration) = row?;
            let Some(binding) = self.catalog_binding(file)? else {
                continue;
            };
            let Some(first) = binding.items.first() else {
                continue;
            };
            let Some(node) = self.catalog_node(&first.media)? else {
                continue;
            };
            let Some(work) = self.catalog_node(&node.root)? else {
                continue;
            };
            if !seen.insert(work.id.clone()) {
                continue;
            }
            let episode = binding
                .playing_item(position, duration)
                .map(|item| self.catalog_node(&item.media))
                .transpose()?
                .flatten();
            result.push(MediaWatching {
                file: crate::catalog::FileResume {
                    file,
                    url,
                    position,
                    duration,
                },
                work,
                episode,
                still,
            });
        }
        Ok(result)
    }
}

#[path = "catalog/requests.rs"]
mod requests;

#[cfg(test)]
mod tests;
