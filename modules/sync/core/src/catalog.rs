//! Current-schema media records, keyed individually rather than as a whole-library
//! blob. Transport UUIDs derive from typed TMDB references; local row ids and CAS
//! counters, playback samples, jobs and account outboxes never leave the device.
use crate::{
    format::{Op, State},
    repository::SyncError,
    value::Value,
};
#[cfg(test)]
mod tests;
use erika_store::{Db, catalog::CatalogSnapshot, portable::FileKey};

pub const NODES: &str = "media.nodes";
pub const LINKS: &str = "media.providers";
pub const BINDINGS: &str = "media.bindings";
pub const DANMAKU: &str = "media.danmaku";
pub const WATCHED: &str = "media.watched";
pub const MARKS: &str = "media.marks";
pub const CATEGORIES: &[&str] = &[NODES, LINKS, BINDINGS, DANMAKU, WATCHED, MARKS];

fn error() -> SyncError {
    SyncError::new("媒体同步记录格式不正确")
}

pub fn records(snapshot: &CatalogSnapshot) -> Result<State, SyncError> {
    let snapshot = snapshot.for_sync().map_err(|e| SyncError::new(e.message))?;
    let mut state = State::new();
    let mut put = |category: &str,
                   key: String,
                   data: serde_json::Value,
                   manual: bool|
     -> Result<(), SyncError> {
        let at = data.get("updated_at").and_then(serde_json::Value::as_i64);
        let mut value = Value::map([
            ("data", Value::from_json(&data)),
            ("manual", Value::Bool(manual)),
        ]);
        if let (Value::Map(fields), Some(at)) = (&mut value, at) {
            fields.insert("at".into(), Value::Int(at));
        }
        if state
            .entry(category.into())
            .or_default()
            .insert(key, value)
            .is_some()
        {
            return Err(error());
        }
        Ok(())
    };
    for n in snapshot.nodes {
        put(
            NODES,
            n.id.to_string(),
            serde_json::to_value(&n).map_err(|_| error())?,
            false,
        )?;
    }
    for l in snapshot.links {
        let key = serde_json::json!([l.media, l.reference]).to_string();
        put(
            LINKS,
            key,
            serde_json::to_value(&l).map_err(|_| error())?,
            l.manual,
        )?;
    }
    for b in snapshot.bindings {
        let data = serde_json::to_value(&b).map_err(|_| error())?;
        let manual = data["binding"]["decision"] != "auto";
        put(BINDINGS, b.file.text(), data, manual)?;
    }
    for (file, b) in snapshot.danmaku {
        put(DANMAKU, file.text(), serde_json::json!([file, b]), b.manual)?;
    }
    for w in snapshot.watched {
        put(
            WATCHED,
            w.media.to_string(),
            serde_json::to_value(&w).map_err(|_| error())?,
            w.manual,
        )?;
    }
    for m in snapshot.marks {
        let key = serde_json::json!([m.media, m.reference]).to_string();
        put(
            MARKS,
            key,
            serde_json::to_value(&m).map_err(|_| error())?,
            true,
        )?;
    }
    Ok(state)
}

/// Validate keys and the envelope as well as the payload. A forged `manual`/`at`
/// field must not win a merge independently of the record it describes.
pub fn validate(state: &State) -> Result<(), SyncError> {
    let json = |category: &str| -> Result<Vec<serde_json::Value>, SyncError> {
        state
            .get(category)
            .into_iter()
            .flat_map(|m| m.values())
            .map(|v| v.get("data").map(Value::to_json).ok_or_else(error))
            .collect()
    };
    let snapshot: CatalogSnapshot = serde_json::from_value(serde_json::json!({
        "nodes": json(NODES)?, "links": json(LINKS)?, "bindings": json(BINDINGS)?,
        "danmaku": json(DANMAKU)?, "watched": json(WATCHED)?, "marks": json(MARKS)?,
    }))
    .map_err(|_| error())?;
    let canonical = records(&snapshot)?;
    for category in CATEGORIES {
        if canonical.get(*category) != state.get(*category).filter(|m| !m.is_empty()) {
            return Err(error());
        }
    }
    Ok(())
}

/// Distinct manual mappings have no shared clock. Report their conflict instead
/// of quietly replacing either user's decision. A causal change (the other side
/// still equals the common base) is not a conflict.
pub fn check_merge(base: Option<&State>, remote: &State, local: &State) -> Result<(), SyncError> {
    validate(remote)?;
    validate(local)?;
    for category in [LINKS, BINDINGS, DANMAKU] {
        if let (Some(a), Some(b)) = (remote.get(category), local.get(category)) {
            for (key, theirs) in a {
                let Some(ours) = b.get(key) else { continue };
                let before = base.and_then(|s| s.get(category)).and_then(|m| m.get(key));
                if theirs != ours
                    && before != Some(theirs)
                    && before != Some(ours)
                    && is_manual(theirs)
                    && is_manual(ours)
                {
                    return Err(SyncError::new("不同设备的人工媒体绑定冲突，请先统一绑定"));
                }
            }
        }
    }
    Ok(())
}

/// Merge cached metadata before computing either patch. Storage retains absent
/// fields on a partial refresh; the agreed base must describe that same result.
pub fn merge(base: Option<&State>, remote: &State, local: &State) -> Result<State, SyncError> {
    check_merge(base, remote, local)?;
    let mut merged = match base {
        Some(base) => crate::merge::merge_three(base, remote, local),
        None => crate::merge::merge_first(remote, local),
    };
    if let Some(nodes) = merged.get_mut(NODES) {
        for (id, value) in nodes {
            let decode = |value: &Value| -> Result<erika_catalog::MediaNode, SyncError> {
                serde_json::from_value(value.get("data").ok_or_else(error)?.to_json())
                    .map_err(|_| error())
            };
            let mut node = decode(value)?;
            for state in [local, remote] {
                if let Some(previous) = state.get(NODES).and_then(|nodes| nodes.get(id)) {
                    node.retain_metadata(&decode(previous)?);
                }
            }
            let Value::Map(fields) = value else { return Err(error()) };
            fields.insert("data".into(), Value::from_json(&serde_json::to_value(node).map_err(|_| error())?));
        }
    }
    validate(&merged)?;
    Ok(merged)
}

pub fn is_manual(value: &Value) -> bool {
    value.get("manual").and_then(Value::as_bool) == Some(true)
}

pub fn apply(db: &Db, expected: &CatalogSnapshot, ops: &[Op]) -> Result<(), SyncError> {
    let before = records(expected)?;
    let mut after = before.clone();
    crate::merge::apply(
        &mut after,
        ops.iter()
            .filter(|op| CATEGORIES.contains(&op.category.as_str()))
            .cloned(),
    );
    validate(&after)?;
    let mut removed_bindings = Vec::new();
    let mut removed_danmaku = Vec::new();
    for op in ops
        .iter()
        .filter(|op| CATEGORIES.contains(&op.category.as_str()) && op.value.is_none())
    {
        let removed = match op.category.as_str() {
            BINDINGS => &mut removed_bindings,
            DANMAKU => &mut removed_danmaku,
            // Media identities, provider links and explicit watched/mark values
            // are retained records. Clearing a value is a normal record update.
            _ => return Err(SyncError::new("媒体状态需要显式记录，不能从同步文件中删除")),
        };
        removed.push(FileKey::parse(&op.key).ok_or_else(error)?);
    }
    let data = |category: &str| -> Vec<serde_json::Value> {
        after
            .get(category)
            .into_iter()
            .flat_map(|m| m.iter())
            .filter(|(key, value)| {
                category == NODES || before.get(category).and_then(|m| m.get(*key)) != Some(*value)
            })
            .map(|(_, value)| value.get("data").expect("validated").to_json())
            .collect()
    };
    let snapshot: CatalogSnapshot = serde_json::from_value(serde_json::json!({
        "nodes": data(NODES), "links": data(LINKS), "bindings": data(BINDINGS),
        "danmaku": data(DANMAKU), "watched": data(WATCHED), "marks": data(MARKS),
    }))
    .map_err(|_| error())?;
    db.catalog_import_checked(&snapshot, expected, &removed_bindings, &removed_danmaku)
        .map_err(|e| SyncError::new(e.message))
}
