//! The plugin v2 catalogue boundary. Reads use typed identities; mutations
//! acknowledge the same validated store transactions as the native UI and CLI.
use super::*;
use erika_catalog::{Kind, MediaId, Provider, ProviderRef, Relation, Watched};
use erika_store::catalog::{CatalogBatch, Error, Result};

fn id(value: Option<&Value>) -> Result<MediaId> {
    Ok(MediaId::parse(
        value
            .and_then(Value::as_str)
            .ok_or_else(|| Error::invalid("缺少媒体标识"))?,
    )?)
}
fn text<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::invalid("请求字段格式不正确"))
}
fn flag(value: &Value, field: &str) -> Result<bool> {
    value
        .get(field)
        .and_then(Value::as_bool)
        .ok_or_else(|| Error::invalid("请求字段格式不正确"))
}
fn permission(method: &str) -> Option<&'static str> {
    match method {
        "getMedia" | "getFiles" | "getChildren" | "getWatched" => Some("library.read"),
        "apply" | "undo" | "setWatched" => Some("library.write"),
        "registerSource" | "beginScan" | "upsertItems" | "finishScan" | "getSourceItems"
        | "getSourceItem" | "getSourceStatus" => Some("library.sources"),
        _ => None,
    }
}

pub(super) fn attach_context(data: &mut Value, snapshot: &Value) {
    let (Some(data), Some(context)) = (data.as_object_mut(), snapshot.get("library.mediaContext"))
    else {
        return;
    };
    if context.get("ok") == Some(&Value::Bool(true)) {
        data.insert("mediaContext".into(), context["value"].clone());
    } else {
        data.insert("mediaContextError".into(), context["error"].clone());
    }
}

impl ErikaEngine {
    /// Recheck permissions in Rust: scripts may forge their JavaScript outbox.
    pub(super) fn catalogue_call(
        &mut self,
        plugin: &str,
        method: &str,
        input: &Value,
    ) -> Result<Value> {
        let required = permission(method).ok_or_else(|| Error::invalid("未知媒体库操作"))?;
        if !self
            .plugins
            .find(plugin)
            .is_some_and(|p| p.pack.has(required))
        {
            return Err(Error {
                code: "permission_denied".into(),
                message: format!("插件没有声明 {required} 权限"),
            });
        }
        if input.to_string().len() > 4 * 1024 * 1024 {
            return Err(Error::invalid("请求过大"));
        }
        let value = match method {
            "registerSource" | "beginScan" | "upsertItems" | "finishScan" | "getSourceItems"
            | "getSourceItem" | "getSourceStatus" => {
                self.source_catalogue_call(plugin, method, input)?
            }
            "getMedia" => match (input.get("id"), input.get("reference")) {
                (Some(value), None) => json!([self
                    .db
                    .catalog_node(&id(Some(value))?)?
                    .ok_or_else(|| Error::invalid("媒体不存在"))?]),
                (None, Some(value)) => {
                    let reference: ProviderRef = serde_json::from_value(value.clone())?;
                    json!(self.db.catalog_by_ref(&reference)?)
                }
                _ => return Err(Error::invalid("请指定媒体标识或提供方引用")),
            },
            "getFiles" | "getChildren" | "getWatched" => {
                let media = id(Some(input))?;
                self.db
                    .catalog_node(&media)?
                    .ok_or_else(|| Error::invalid("媒体不存在"))?;
                match method {
                    "getFiles" => json!(self.db.catalog_files(&media)?),
                    "getChildren" => json!(self.db.catalog_children(&media)?),
                    _ => json!(self.db.catalog_watched(&media)?),
                }
            }
            "apply" => {
                let request = text(input, "request")?;
                let batch: CatalogBatch = serde_json::from_value(
                    input
                        .get("batch")
                        .cloned()
                        .ok_or_else(|| Error::invalid("缺少修改内容"))?,
                )?;
                let preview = flag(input, "preview")?;
                let receipt = self.db.catalog_request(request, &batch, preview)?;
                if !preview {
                    self.catalogue_changed();
                }
                json!({"preview":preview,"receipt":receipt})
            }
            "undo" => {
                self.db.catalog_undo(
                    input
                        .as_str()
                        .ok_or_else(|| Error::invalid("缺少修改标识"))?,
                )?;
                self.catalogue_changed();
                Value::Null
            }
            "setWatched" => {
                let media = id(input.get("mediaId"))?;
                let watched = flag(input, "watched")?;
                let revision = input
                    .get("revision")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| Error::invalid("缺少观看状态版本"))?;
                let result = self.db.catalog_set_watched(&Watched {
                    media,
                    watched,
                    manual: true,
                    updated_at: erika_store::now(),
                    revision,
                })?;
                self.catalogue_changed();
                json!(result)
            }
            _ => unreachable!(),
        };
        Ok(value)
    }

    pub(super) fn plugin_catalogue_request(&mut self, plugin: &str, args: &[Value]) {
        let Some(request) = args.first().and_then(Value::as_u64) else {
            return;
        };
        let method = args.get(1).and_then(Value::as_str).unwrap_or_default();
        let input = args.get(2).unwrap_or(&Value::Null);
        let result = match self.catalogue_call(plugin, method, input) {
            Ok(value) => json!({"ok":true,"value":value}),
            Err(error) => json!({"ok":false,"error":{"code":error.code,"message":error.message}}),
        };
        let snapshot = self.plugin_snapshot(plugin);
        if let Some(runtime) = self.plugins.runtimes.get_mut(plugin) {
            runtime.settle(request, Ok(result), &snapshot);
        }
    }

    pub(super) fn catalogue_changed(&mut self) {
        if let Some(file) = self.file {
            if let Err(error) = self.show_catalog(file) {
                self.notify(Notice::OperationFailed {
                    reason: error.message,
                });
            }
        }
        #[cfg(feature = "bern-module-anime")]
        erika_anime_protocol::anime::calls::history_changed();
        #[cfg(feature = "bern-module-library")]
        erika_library_protocol::library::calls::history_changed();
    }

    /// Local catalogue context, never a URL, request header or account secret.
    pub(super) fn plugin_media_context(&self) -> Result<Value> {
        let Some(file) = self.file else {
            return Ok(Value::Null);
        };
        let context = self.db.catalog_context(file)?;
        let playing = context
            .playing(self.state.elapsed, self.state.duration)
            .cloned();
        let mut value = serde_json::to_value(context)?;
        value["version"] = json!(2);
        value["playing"] = json!(playing);
        Ok(value)
    }

    /// The old numeric namespace contains only unambiguous, real DDP subjects.
    pub(super) fn canonical_shows_for_plugins(
        &self,
    ) -> Result<BTreeMap<i64, (String, std::collections::BTreeSet<String>)>> {
        let mut shows: BTreeMap<i64, (String, std::collections::BTreeSet<String>)> =
            BTreeMap::new();
        for shelf in self.db.catalog_shelf(None)? {
            let mut scopes = vec![shelf.node.clone()];
            if shelf.node.kind == Kind::Tv {
                scopes.extend(self.db.catalog_children(&shelf.node.id)?);
            }
            for node in scopes {
                let links: Vec<_> = self
                    .db
                    .catalog_links(&node.id)?
                    .into_iter()
                    .filter(|l| {
                        l.confirmed
                            && l.relation == Relation::Exact
                            && l.reference.provider == Provider::Dandanplay
                            && l.reference.kind == Kind::Subject
                            && l.reference.scope.is_empty()
                    })
                    .collect();
                if links.len() != 1 {
                    continue;
                }
                let link = &links[0];
                let Some(external) = link.reference.id.parse::<i64>().ok().filter(|id| *id > 0)
                else {
                    continue;
                };
                if self.db.catalog_by_ref(&link.reference)?.len() != 1 {
                    continue;
                }
                let files = self.db.catalog_files(&node.id)?;
                if files.is_empty() {
                    continue;
                }
                let row = shows
                    .entry(external)
                    .or_insert_with(|| (node.title.clone(), Default::default()));
                for file in files {
                    if let Some(folder) = bern::files::file_path(&file.url)
                        .and_then(|p| p.parent().map(|p| p.display().to_string()))
                    {
                        row.1.insert(folder);
                    }
                }
            }
        }
        Ok(shows)
    }
}

#[cfg(test)]
mod tests;
