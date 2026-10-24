//! The market: the plugins its indexes list, and fetching one into the
//! plugins folder. NipaPlay's market lists single scripts; Erika's lists
//! folders of `pack.json`, the script and `icon.png`. Until Erika's market
//! is published its index answers nothing, and only NipaPlay's shows.

use super::pack::Pack;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// NipaPlay's market.
pub const NIPAPLAY_INDEX: &str =
    "https://raw.githubusercontent.com/AimesSoft/Nipaplay-plugins/refs/heads/main/plugins.json";
/// Erika's own market, once it is published.
pub const ERIKA_INDEX: &str =
    "https://raw.githubusercontent.com/AimesSoft/erika_plugins/refs/heads/main/plugins.json";
/// More indexes, separated by `;`: addresses or local `plugins.json` files,
/// for trying a market before it is published; `-` reads none at all.
pub const INDEX_VARIABLE: &str = "ERIKA_PLUGIN_INDEX";
/// A folder to install plugins in, in place of the application's own.
pub const FOLDER_VARIABLE: &str = "ERIKA_PLUGIN_DIR";

/// A plugin a market lists.
#[derive(Debug, Clone, PartialEq)]
pub struct Listing {
    pub pack: Pack,
    pub files: Files,
    /// Its icon, where the market keeps it.
    pub image: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Files {
    /// A NipaPlay script.
    Script(String),
    /// An Erika folder: its address, holding `pack.json`.
    Folder(String),
}

/// The indexes to read.
pub fn indexes() -> Vec<String> {
    let mut indexes = vec![ERIKA_INDEX.to_string(), NIPAPLAY_INDEX.to_string()];
    if let Ok(more) = std::env::var(INDEX_VARIABLE) {
        if more.trim() == "-" {
            return Vec::new();
        }
        indexes.extend(
            more.split(';')
                .map(str::trim)
                .filter(|index| !index.is_empty())
                .map(String::from),
        );
    }
    indexes
}

/// Read an address: http(s), `file://`, or a local path.
#[cfg(not(target_arch = "wasm32"))]
pub fn get(address: &str) -> Result<Vec<u8>, String> {
    if address.starts_with("https://") || address.starts_with("http://") {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(20)))
            .build()
            .into();
        let mut response = agent
            .get(address)
            .call()
            .map_err(|error| format!("{address}：{error}"))?;
        return response
            .body_mut()
            .with_config()
            .limit(8 << 20)
            .read_to_vec()
            .map_err(|error| error.to_string());
    }
    let path = bern::files::file_path(address).unwrap_or_else(|| PathBuf::from(address));
    std::fs::read(&path).map_err(|error| format!("{}：{error}", path.display()))
}

#[cfg(target_arch = "wasm32")]
pub fn get(_address: &str) -> Result<Vec<u8>, String> {
    Err("网页版暂时连不上插件市场".into())
}

/// The address `relative` names beside `base`, the index or folder it is
/// listed in.
pub fn beside(base: &str, relative: &str) -> String {
    if relative.contains("://") || relative.starts_with('/') {
        return relative.to_string();
    }
    let folder = base.rsplit_once('/').map_or("", |(folder, _)| folder);
    format!("{folder}/{}", relative.trim_start_matches("./"))
}

/// The permissions a NipaPlay script declares, read from its text: its
/// index does not list them, and the market tags plugins by them.
pub fn declared_permissions(script: &str) -> Vec<String> {
    let Some(start) = script.find("permissions") else {
        return Vec::new();
    };
    let rest = &script[start..];
    let (Some(open), Some(close)) = (rest.find('['), rest.find(']')) else {
        return Vec::new();
    };
    if close < open {
        return Vec::new();
    }
    rest[open + 1..close]
        .split(',')
        .map(|item| item.trim().trim_matches(|c| c == '\'' || c == '"' || c == '`'))
        .filter(|item| !item.is_empty())
        .map(String::from)
        .collect()
}

/// The plugins one index lists.
pub fn read_index(address: &str) -> Result<Vec<Listing>, String> {
    let bytes = get(address)?;
    let index: Value =
        serde_json::from_slice(&bytes).map_err(|error| format!("{address}：{error}"))?;
    let entries = index
        .get("plugins")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{address} 里没有 plugins"))?;
    let mut listings = Vec::new();
    for entry in entries {
        // NipaPlay lists a script by `file`; Erika a folder by `path`.
        if let Some(file) = entry.get("file").and_then(Value::as_str) {
            let Ok(mut pack) = Pack::from_manifest(entry) else {
                continue;
            };
            let script = beside(address, file);
            if let Ok(text) = get(&script).map(|bytes| String::from_utf8_lossy(&bytes).into_owned()) {
                pack = Pack::from_manifest(&serde_json::json!({
                    "id": pack.id, "name": pack.name, "version": pack.version,
                    "description": pack.description, "author": pack.author,
                    "github": pack.github, "permissions": declared_permissions(&text),
                }))
                .unwrap_or(pack);
            }
            listings.push(Listing {
                pack,
                files: Files::Script(script),
                image: None,
            });
        } else if let Some(path) = entry.get("path").and_then(Value::as_str) {
            let Ok(pack) = Pack::from_pack(entry) else {
                continue;
            };
            let folder = beside(address, path.trim_end_matches('/'));
            let image = pack
                .image
                .as_ref()
                .map(|image| format!("{folder}/{image}"));
            listings.push(Listing {
                pack,
                files: Files::Folder(folder),
                image,
            });
        }
    }
    Ok(listings)
}

/// Every index's plugins, the first index's where two list the same one,
/// and why the indexes that failed did. Erika's market is not published
/// yet: its failure is not reported.
pub fn read_all() -> (Vec<Listing>, Vec<String>) {
    let mut listings: Vec<Listing> = Vec::new();
    let mut errors = Vec::new();
    for index in indexes() {
        match read_index(&index) {
            Ok(found) => {
                for listing in found {
                    if !listings.iter().any(|known| known.pack.id == listing.pack.id) {
                        listings.push(listing);
                    }
                }
            }
            Err(_) if index == ERIKA_INDEX => {}
            Err(error) => errors.push(error),
        }
    }
    (listings, errors)
}

/// Where plugins are installed, each in a folder named by its id.
pub fn folder() -> Option<PathBuf> {
    if let Some(folder) = std::env::var_os(FOLDER_VARIABLE) {
        return Some(PathBuf::from(folder));
    }
    bern::storage::data_dir().map(|dir| dir.join("plugins"))
}

/// Fetch `listing` and install it, in place of an installed version.
pub fn install(listing: &Listing) -> Result<Pack, String> {
    match &listing.files {
        Files::Script(address) => {
            let script = get(address)?;
            install_script(&listing.pack, &script)
        }
        Files::Folder(address) => {
            let pack_json = get(&format!("{address}/pack.json"))?;
            let declared: Value = serde_json::from_slice(&pack_json)
                .map_err(|error| format!("pack.json：{error}"))?;
            let pack = Pack::from_pack(&declared)?;
            let script = get(&format!("{address}/{}", pack.main))?;
            let image = match &pack.image {
                Some(image) => Some(get(&format!("{address}/{image}"))?),
                None => None,
            };
            place(&pack, &script, image.as_deref())
        }
    }
}

/// Install a NipaPlay script under the declaration it was listed with,
/// which its own `pluginManifest` replaces when it loads.
pub fn install_script(pack: &Pack, script: &[u8]) -> Result<Pack, String> {
    let mut pack = pack.clone();
    pack.main = "main.js".into();
    place(&pack, script, None)
}

/// Install a plugin the user picked: a folder with `pack.json`, or a
/// NipaPlay script whose declaration `declared` read from it.
pub fn import_folder(path: &Path) -> Result<Pack, String> {
    let declared: Value = serde_json::from_slice(
        &std::fs::read(path.join("pack.json")).map_err(|_| "文件夹里没有 pack.json".to_string())?,
    )
    .map_err(|error| format!("pack.json：{error}"))?;
    let pack = Pack::from_pack(&declared)?;
    let script =
        std::fs::read(path.join(&pack.main)).map_err(|_| format!("文件夹里没有 {}", pack.main))?;
    let image = match &pack.image {
        Some(image) => Some(std::fs::read(path.join(image)).map_err(|_| format!("文件夹里没有 {image}"))?),
        None => None,
    };
    place(&pack, &script, image.as_deref())
}

/// Write the plugin's files into its folder, replacing what was there.
fn place(pack: &Pack, script: &[u8], image: Option<&[u8]>) -> Result<Pack, String> {
    let root = folder().ok_or("没有可以安装插件的地方")?;
    let staging = root.join(format!(".{}.new", pack.id));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|error| error.to_string())?;
    let write = |name: &str, bytes: &[u8]| {
        std::fs::write(staging.join(name), bytes).map_err(|error| error.to_string())
    };
    write(&pack.main, script)?;
    if let (Some(name), Some(bytes)) = (&pack.image, image) {
        write(name, bytes)?;
    }
    write(
        "pack.json",
        serde_json::to_string_pretty(&pack.to_value()).unwrap().as_bytes(),
    )?;
    let target = root.join(&pack.id);
    let _ = std::fs::remove_dir_all(&target);
    std::fs::rename(&staging, &target).map_err(|error| error.to_string())?;
    Ok(pack.clone())
}

/// The plugins installed, each with its folder.
pub fn installed() -> Vec<(Pack, PathBuf)> {
    let Some(root) = folder() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut found: Vec<(Pack, PathBuf)> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter(|path| !path.file_name().is_some_and(|name| name.to_string_lossy().starts_with('.')))
        .filter_map(|path| {
            let text = std::fs::read(path.join("pack.json")).ok()?;
            let pack = Pack::from_installed(&serde_json::from_slice(&text).ok()?).ok()?;
            Some((pack, path))
        })
        .collect();
    found.sort_by(|a, b| a.0.priority.cmp(&b.0.priority).then(a.0.id.cmp(&b.0.id)));
    found
}

pub fn remove(id: &str) {
    if let Some(root) = folder() {
        let _ = std::fs::remove_dir_all(root.join(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_resolve_beside_their_index() {
        assert_eq!(
            beside("https://host/repo/plugins.json", "plugins/a/a.js"),
            "https://host/repo/plugins/a/a.js"
        );
        assert_eq!(beside("/local/plugins.json", "./b"), "/local/b");
    }

    #[test]
    fn scripts_declare_their_permissions() {
        let script = "const pluginManifest = {\n  id: 'x',\n  permissions: ['danmaku.modify', \"ui.dialog\"],\n};";
        assert_eq!(declared_permissions(script), vec!["danmaku.modify", "ui.dialog"]);
        assert!(declared_permissions("const a = 1;").is_empty());
    }
}
