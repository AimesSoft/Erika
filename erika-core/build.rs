//! A debug build packs the plugins of the local `erika_plugins` folder, the
//! market's repository beside this one, into `.epk` packages the
//! market lists first: while the market is not published, every test
//! device sees them, a phone as much as this computer. `ERIKA_LOCAL_PLUGINS`
//! names another folder, or `-` none; a release build packs nothing.

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=ERIKA_LOCAL_PLUGINS");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let mut list = String::from("&[\n");
    if let Some(root) = folder() {
        let index = root.join("plugins.json");
        println!("cargo:rerun-if-changed={}", index.display());
        let entries = std::fs::read(&index)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|index| index.get("plugins").and_then(|p| p.as_array()).cloned())
            .unwrap_or_default();
        for entry in entries {
            let (Some(id), Some(path)) = (
                entry.get("id").and_then(|v| v.as_str()),
                entry.get("path").and_then(|v| v.as_str()),
            ) else {
                continue;
            };
            let plugin = root.join(path);
            println!("cargo:rerun-if-changed={}", plugin.display());
            let package = out.join(format!("{id}.epk"));
            if pack(&plugin, &package).is_ok() {
                writeln!(list, "    ({id:?}, include_bytes!({:?})),", package.display().to_string()).unwrap();
            }
        }
    }
    list.push(']');
    std::fs::write(
        out.join("local_plugins.rs"),
        format!("pub const LOCAL_PLUGINS: &[(&str, &[u8])] = {list};\n"),
    )
    .unwrap();
}

/// The local plugins' folder, in a debug build only.
fn folder() -> Option<PathBuf> {
    if std::env::var("PROFILE").as_deref() != Ok("debug") {
        return None;
    }
    let folder = match std::env::var("ERIKA_LOCAL_PLUGINS") {
        Ok(folder) if folder.trim() == "-" => return None,
        Ok(folder) => PathBuf::from(folder),
        Err(_) => Path::new(env!("CARGO_MANIFEST_DIR")).join("../../erika_plugins"),
    };
    folder.join("plugins.json").is_file().then_some(folder)
}

/// Zip the plugin's folder, its files at the archive's root.
fn pack(plugin: &Path, package: &Path) -> std::io::Result<()> {
    let mut zip = zip::ZipWriter::new(std::fs::File::create(package)?);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut files = Vec::new();
    collect(plugin, plugin, &mut files)?;
    files.sort();
    for (name, path) in files {
        zip.start_file(name, options)?;
        zip.write_all(&std::fs::read(path)?)?;
    }
    zip.finish()?;
    Ok(())
}

fn collect(root: &Path, dir: &Path, files: &mut Vec<(String, PathBuf)>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.file_name().is_some_and(|name| name.to_string_lossy().starts_with('.')) {
            continue;
        }
        if path.is_dir() {
            collect(root, &path, files)?;
        } else {
            let name = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            files.push((name, path));
        }
    }
    Ok(())
}
