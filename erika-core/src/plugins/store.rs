//! The market: the plugins its indexes list, and fetching one into the
//! plugins folder. NipaPlay's market lists single scripts; Erika's lists
//! folders of `pack.json`, the script and `icon.png`. Until Erika's market
//! is published its index answers nothing, and only NipaPlay's shows; a
//! debug build lists the local `erika_plugins` folder's plugins first
//! (`build.rs`).
//!
//! A plugin handed around on its own is a package, `.epk`: a zip archive of
//! its folder, `pack.json` at its root. Installed, it is that folder again.

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
/// The extension of a plugin package.
pub const PACKAGE_EXTENSION: &str = "epk";
/// The most a package may hold unpacked, against archives that inflate
/// without end.
const PACKAGE_LIMIT: u64 = 32 << 20;

mod local {
    // The packages of the local `erika_plugins` folder, by id; none in a
    // release build.
    include!(concat!(env!("OUT_DIR"), "/local_plugins.rs"));
}

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
    /// A package of the local `erika_plugins` folder this build carries.
    Local(&'static [u8]),
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
    if relative.contains("://") || relative.starts_with('/') || Path::new(relative).is_absolute() {
        return relative.to_string();
    }
    // A local index on Windows has its folder written with backslashes.
    let folder = base.rfind(['/', '\\']).map_or("", |at| &base[..at]);
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
    let mut listings: Vec<Listing> = local_listings();
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

/// The plugins of the local `erika_plugins` folder this build carries, each
/// with its icon written out where the market can show it.
fn local_listings() -> Vec<Listing> {
    local::LOCAL_PLUGINS
        .iter()
        .filter_map(|(id, package)| {
            let mut archive = zip::ZipArchive::new(std::io::Cursor::new(*package)).ok()?;
            let declared: Value = serde_json::from_slice(&read_entry(&mut archive, "pack.json").ok()?).ok()?;
            let pack = Pack::from_pack(&declared).ok()?;
            let image = pack.image.as_ref().and_then(|name| {
                let path = folder()?.join(".icons").join(format!("{id}.png"));
                std::fs::create_dir_all(path.parent()?).ok()?;
                std::fs::write(&path, read_entry(&mut archive, name).ok()?).ok()?;
                Some(bern::prelude::MediaSource::file(path).url)
            });
            Some(Listing { pack, files: Files::Local(package), image })
        })
        .collect()
}

/// One file of a package, whole.
pub fn package_file(package: &[u8], name: &str) -> Result<Vec<u8>, String> {
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(package)).map_err(|_| "这不是 Erika 插件包".to_string())?;
    read_entry(&mut archive, name)
}

/// One file of an opened package, whole.
fn read_entry<R: std::io::Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let file = archive.by_name(name).map_err(|_| format!("插件包里没有 {name}"))?;
    let mut bytes = Vec::new();
    file.take(PACKAGE_LIMIT).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    Ok(bytes)
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
            let readme = get(&beside(address, "README.md")).ok();
            install_script(&listing.pack, &script, readme.as_deref())
        }
        Files::Local(package) => unpack(package),
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
            let readme = get(&format!("{address}/README.md")).ok();
            place(&pack, &script, image.as_deref(), readme.as_deref())
        }
    }
}

/// Install a NipaPlay script under the declaration it was listed with,
/// which its own `pluginManifest` replaces when it loads, with the README
/// beside it.
pub fn install_script(pack: &Pack, script: &[u8], readme: Option<&[u8]>) -> Result<Pack, String> {
    let mut pack = pack.clone();
    pack.main = "main.js".into();
    place(&pack, script, None, readme)
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
    let readme = std::fs::read(path.join("README.md")).ok();
    place(&pack, &script, image.as_deref(), readme.as_deref())
}

/// Install a package the user picked.
pub fn import_package(path: &Path) -> Result<Pack, String> {
    unpack(&std::fs::read(path).map_err(|error| format!("{}：{error}", path.display()))?)
}

/// Unpack a package into its plugin's folder, replacing what was there:
/// every file it holds, under the id its `pack.json` declares, once its
/// script is there and no name leads out of the folder.
pub fn unpack(package: &[u8]) -> Result<Pack, String> {
    unpack_into(&folder().ok_or("没有可以安装插件的地方")?, package)
}

fn unpack_into(root: &Path, package: &[u8]) -> Result<Pack, String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(package))
        .map_err(|_| "这不是 Erika 插件包".to_string())?;
    let declared: Value = serde_json::from_slice(&read_entry(&mut archive, "pack.json")?)
        .map_err(|error| format!("pack.json：{error}"))?;
    let pack = Pack::from_pack(&declared)?;
    if archive.index_for_name(&pack.main).is_none() {
        return Err(format!("插件包里没有 {}", pack.main));
    }
    let staging = root.join(format!(".{}.new", pack.id));
    let _ = std::fs::remove_dir_all(&staging);
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|error| error.to_string())?;
        let Some(name) = file.enclosed_name() else {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(format!("插件包里的 {} 不在插件的文件夹里", file.name()));
        };
        let target = staging.join(name);
        if file.is_dir() {
            std::fs::create_dir_all(&target).map_err(|error| error.to_string())?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut out = std::fs::File::create(&target).map_err(|error| error.to_string())?;
        // What is written counts, whatever the archive says a file holds.
        total += std::io::copy(&mut std::io::Read::take(&mut file, PACKAGE_LIMIT - total + 1), &mut out)
            .map_err(|error| error.to_string())?;
        if total > PACKAGE_LIMIT {
            let _ = std::fs::remove_dir_all(&staging);
            return Err("插件包太大".into());
        }
    }
    let target = root.join(&pack.id);
    let _ = std::fs::remove_dir_all(&target);
    std::fs::rename(&staging, &target).map_err(|error| error.to_string())?;
    Ok(pack)
}

/// Write the plugin's files into its folder, replacing what was there.
fn place(pack: &Pack, script: &[u8], image: Option<&[u8]>, readme: Option<&[u8]>) -> Result<Pack, String> {
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
    if let Some(bytes) = readme {
        write("README.md", bytes)?;
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
        assert_eq!(
            beside(r"C:\Users\me\erika_plugins\plugins.json", "plugins/a"),
            r"C:\Users\me\erika_plugins/plugins/a"
        );
    }

    /// A package of `files`, by name and text.
    fn package(files: &[(&str, &str)]) -> Vec<u8> {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, text) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    const PACK: &str = r#"{"id": "test.pack", "name": "测试", "version": "1.0.0", "category": "tools", "main": "main.js"}"#;

    #[test]
    fn packages_unpack_into_the_plugins_folder() {
        let root = std::env::temp_dir().join(format!("erika-unpack-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let pack = unpack_into(
            &root,
            &package(&[("pack.json", PACK), ("main.js", "1;"), ("lib/util.js", "2;")]),
        )
        .unwrap();
        assert_eq!(pack.id, "test.pack");
        assert_eq!(std::fs::read_to_string(root.join("test.pack/main.js")).unwrap(), "1;");
        assert_eq!(std::fs::read_to_string(root.join("test.pack/lib/util.js")).unwrap(), "2;");
        // Installed again, it replaces what was there.
        unpack_into(&root, &package(&[("pack.json", PACK), ("main.js", "3;")])).unwrap();
        assert!(!root.join("test.pack/lib").exists());
        // A name that leads out of the folder, and a package without its
        // script, install nothing.
        let outside = package(&[("pack.json", PACK), ("main.js", "1;"), ("../escape.js", "x")]);
        assert!(unpack_into(&root, &outside).is_err());
        assert!(!root.join("escape.js").exists());
        assert!(unpack_into(&root, &package(&[("pack.json", PACK)])).is_err());
        assert!(unpack_into(&root, b"not a zip").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_debug_build_lists_the_local_plugins() {
        // Each package the build carries declares the id it is listed by.
        for (id, package) in local::LOCAL_PLUGINS {
            let mut archive = zip::ZipArchive::new(std::io::Cursor::new(*package)).unwrap();
            let declared: Value = serde_json::from_slice(&read_entry(&mut archive, "pack.json").unwrap()).unwrap();
            assert_eq!(Pack::from_pack(&declared).unwrap().id, *id);
        }
    }

    #[test]
    fn scripts_declare_their_permissions() {
        let script = "const pluginManifest = {\n  id: 'x',\n  permissions: ['danmaku.modify', \"ui.dialog\"],\n};";
        assert_eq!(declared_permissions(script), vec!["danmaku.modify", "ui.dialog"]);
        assert!(declared_permissions("const a = 1;").is_empty());
    }
}
