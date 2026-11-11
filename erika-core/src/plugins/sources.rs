//! Plugin-owned source import. Caller identity comes from the running host,
//! never from the request; acknowledgements follow the store transaction.
use super::*;
use erika_store::catalog::{Error, Result};
use erika_store::plugin_sources::{Descriptor, Item};
use serde::Deserialize;

pub(super) mod metadata;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceRequest {
    source: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PageRequest {
    scan: String,
    page: u32,
    items: Vec<Item>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FinishRequest {
    scan: String,
    pages: u32,
    complete: bool,
}

impl ErikaEngine {
    pub(super) fn source_catalogue_call(
        &mut self,
        plugin: &str,
        method: &str,
        input: &Value,
    ) -> Result<Value> {
        match method {
            "registerSource" => {
                let descriptor: Descriptor = serde_json::from_value(input.clone())?;
                let id = self.db.plugin_register_source(plugin, &descriptor)?;
                self.catalogue_changed();
                Ok(json!(id))
            }
            "beginScan" => {
                let request: SourceRequest = serde_json::from_value(input.clone())?;
                if request.source <= 0 {
                    return Err(Error::invalid("来源标识不正确"));
                }
                Ok(json!(self.db.plugin_begin_scan(plugin, request.source)?))
            }
            "getSourceItems" => {
                let request = serde_json::from_value(input.clone())?;
                Ok(json!(self.db.plugin_source_page(plugin, &request)?))
            }
            "getSourceItem" | "getSourceStatus" => {
                let request = serde_json::from_value(input.clone())?;
                if method == "getSourceItem" {
                    Ok(json!(self.db.plugin_source_version(plugin, &request)?))
                } else {
                    Ok(json!(self.db.plugin_source_status(plugin, &request)?))
                }
            }
            "upsertItems" => {
                let request: PageRequest = serde_json::from_value(input.clone())?;
                Ok(json!(self.db.plugin_upsert_items(
                    plugin,
                    &request.scan,
                    request.page,
                    &request.items,
                )?))
            }
            "finishScan" => {
                let request: FinishRequest = serde_json::from_value(input.clone())?;
                let scan = self.db.plugin_finish_scan(
                    plugin,
                    &request.scan,
                    request.pages,
                    request.complete,
                )?;
                if request.complete {
                    self.queue_source_metadata(plugin, scan.source)?;
                    self.catalogue_changed();
                }
                Ok(json!(scan))
            }
            _ => Err(Error::invalid("未知来源操作")),
        }
    }
}

#[cfg(test)]
mod tests;
