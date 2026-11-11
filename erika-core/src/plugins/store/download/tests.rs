use super::*;

const INDEX: &[u8] = br#"{"plugins":[]}"#;
const SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"/>"#;

#[test]
fn only_public_official_repository_urls_are_mirrored() {
    assert!(eligible(super::super::NIPAPLAY_INDEX));
    assert!(eligible(super::super::ERIKA_INDEX));
    for url in [
        "http://raw.githubusercontent.com/AimesSoft/erika_plugins/main/a",
        "https://raw.githubusercontent.com.evil/AimesSoft/erika_plugins/main/a",
        "https://user:secret@raw.githubusercontent.com/AimesSoft/erika_plugins/main/a",
        "https://raw.githubusercontent.com:443/AimesSoft/erika_plugins/main/a",
        "https://raw.githubusercontent.com/AimesSoft/erika_plugins_extra/main/a",
        "https://raw.githubusercontent.com/Other/erika_plugins/main/a",
        "https://raw.githubusercontent.com/AimesSoft/erika_plugins/main/../a",
        "https://raw.githubusercontent.com/AimesSoft/erika_plugins/main/%2e%2e/a",
        "https://raw.githubusercontent.com/AimesSoft/erika_plugins/main//a",
        "https://raw.githubusercontent.com/AimesSoft/erika_plugins/main/a?token=secret",
        "https://raw.githubusercontent.com/AimesSoft/erika_plugins/main/a#fragment",
        "https://gh-proxy.com/https://raw.githubusercontent.com/AimesSoft/erika_plugins/main/a",
        "https://bgm.tv/login",
        "file:///tmp/plugins.json",
    ] {
        assert!(!eligible(url), "{url}");
        assert_eq!(candidates(url), vec![url]);
    }
}

#[test]
fn original_first_and_success_stops_without_mirror_requests() {
    let mut calls = Vec::new();
    let bytes = read_with(super::super::ERIKA_INDEX, Resource::Index, |url| {
        calls.push(url.to_string());
        Ok(INDEX.to_vec())
    })
    .unwrap();
    assert_eq!(bytes, INDEX);
    assert_eq!(calls, vec![super::super::ERIKA_INDEX]);
}

#[test]
fn invalid_responses_advance_in_order_once_each() {
    let mut calls = Vec::new();
    let bytes = read_with(super::super::ERIKA_INDEX, Resource::Index, |url| {
        calls.push(url.to_string());
        match calls.len() {
            1 => Err("DNS".into()),
            2 => Ok(b"<!doctype html><html>error</html>".to_vec()),
            3 => Ok(br#"{"error":"rate limited"}"#.to_vec()),
            _ => Ok(INDEX.to_vec()),
        }
    })
    .unwrap();
    assert_eq!(bytes, INDEX);
    assert_eq!(calls, candidates(super::super::ERIKA_INDEX));
}

#[test]
fn all_failures_report_resource_and_sources() {
    let mut count = 0;
    let error = read_with(super::super::ERIKA_INDEX, Resource::Pack, |_| {
        count += 1;
        Err("unavailable".into())
    })
    .unwrap_err();
    assert_eq!(count, 4);
    assert!(error.contains("插件声明"));
    assert!(error.contains("原站"));
    for mirror in MIRRORS {
        assert!(error.contains(mirror));
    }
}

#[test]
fn non_market_requests_never_reach_a_mirror() {
    for address in [
        "https://bgm.tv/login",
        "https://example.com/private?token=secret",
        "/tmp/index.json",
    ] {
        let mut calls = Vec::new();
        assert!(
            read_with(address, Resource::Index, |url| {
                calls.push(url.to_string());
                Err("offline".into())
            })
            .is_err()
        );
        assert_eq!(calls, vec![address]);
    }
}

#[test]
fn validates_each_resource_without_executing_scripts() {
    assert!(validate(Resource::Index, INDEX).is_ok());
    assert!(validate(Resource::Index, b"{}").is_err());
    assert!(
        validate(
            Resource::Pack,
            br#"{"id":"test.pack","name":"Test","version":"1","category":"tools","main":"main.js"}"#
        )
        .is_ok()
    );
    assert!(validate(Resource::Pack, b"{}").is_err());
    assert!(validate(Resource::Script, b"const a = 1;").is_ok());
    assert!(validate(Resource::Script, b" \n").is_err());
    assert!(validate(Resource::Script, &[0xff]).is_err());
    assert!(validate(Resource::Readme, b"# Readme\n<div>Inline HTML</div>").is_ok());
    for kind in [Resource::Script, Resource::Readme] {
        assert!(
            validate(
                kind,
                "\u{feff} \n<!DOCTYPE HTML><html>proxy error</html>".as_bytes()
            )
            .is_err()
        );
    }
    assert_eq!(image_format(SVG).unwrap(), "svg");
    assert!(image_format(b"<html>not a picture</html>").is_err());
    assert!(image_format(b"\x89PNG\r\n\x1a\ntruncated").is_err());
}

#[test]
fn oversized_candidate_is_rejected_before_next_attempt() {
    let mut count = 0;
    let bytes = read_with(super::super::ERIKA_INDEX, Resource::Index, |_| {
        count += 1;
        if count == 1 {
            Ok(vec![b' '; LIMIT as usize + 1])
        } else {
            Ok(INDEX.to_vec())
        }
    })
    .unwrap();
    assert_eq!(count, 2);
    assert_eq!(bytes, INDEX);
}

#[test]
fn cache_validates_reuses_recovers_and_repairs_images() {
    let root = std::env::temp_dir().join(format!("erika-market-cache-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let address = "https://raw.githubusercontent.com/AimesSoft/erika_plugins/main/icon.svg";
    let first = cache_image_with(&root, address, |_| Ok(SVG.to_vec())).unwrap();
    assert_eq!(first.extension().unwrap(), "svg");
    assert_eq!(
        cache_image_with(&root, address, |_| Ok(SVG.to_vec())).unwrap(),
        first
    );
    assert_eq!(
        cache_image_with(&root, address, |_| Err("offline".into())).unwrap(),
        first
    );
    std::fs::write(&first, b"damaged").unwrap();
    assert!(cache_image_with(&root, address, |_| Err("offline".into())).is_none());
    assert_eq!(
        cache_image_with(&root, address, |_| Ok(SVG.to_vec())).unwrap(),
        first
    );
    assert_eq!(std::fs::read(&first).unwrap(), SVG);
    assert!(cache_image_with(&root, "bad", |_| Ok(b"<html>error</html>".to_vec())).is_none());
    assert!(!std::fs::read_dir(&root).unwrap().any(|entry| {
        entry
            .unwrap()
            .path()
            .extension()
            .is_some_and(|ext| ext == "tmp")
    }));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn concurrent_cache_writers_publish_complete_images() {
    let root = std::env::temp_dir().join(format!("erika-market-parallel-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let paths: Vec<_> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let root = &root;
                scope.spawn(move || {
                    cache_image_with(root, "same-icon", |_| Ok(SVG.to_vec())).unwrap()
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect()
    });
    assert!(paths.iter().all(|path| path == &paths[0]));
    assert_eq!(std::fs::read(&paths[0]).unwrap(), SVG);
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
    std::fs::remove_dir_all(root).unwrap();
}

/// Opt-in network verification: only public resources, no plugin runtime.
#[test]
#[ignore]
fn live_market_mirror_resources_and_isolated_install() {
    let root = super::super::folder().expect("isolated plugin directory");
    assert!(
        root.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("erika-market-verify-")
    );
    std::fs::create_dir_all(&root).unwrap();
    for address in [super::super::ERIKA_INDEX, super::super::NIPAPLAY_INDEX] {
        let bytes = read_with(address, Resource::Index, |candidate| {
            if candidate == address {
                Err("simulated original outage".into())
            } else {
                fetch_once(candidate)
            }
        })
        .unwrap();
        let index: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        println!(
            "mirror fallback: {address}, {} plugins",
            index["plugins"].as_array().unwrap().len()
        );
    }
    let listings = super::super::read_index(super::super::ERIKA_INDEX).unwrap();
    let listing = listings
        .iter()
        .find(|listing| matches!(listing.files, super::super::Files::Folder(_)))
        .unwrap();
    let pack = super::super::install(listing).unwrap();
    let folder = root.join(&pack.id);
    assert!(folder.join("pack.json").is_file());
    assert!(folder.join(&pack.main).is_file());
    if let Some(image) = pack.image {
        image_format(&std::fs::read(folder.join(image)).unwrap()).unwrap();
    }
    assert!(folder.join("README.md").is_file());
    println!(
        "isolated install: {}, script downloaded but never executed",
        pack.id
    );
}

#[test]
fn relative_addresses_keep_the_original_repository_base() {
    let base = super::super::ERIKA_INDEX;
    assert_eq!(
        super::super::beside(base, "tools/icon.png"),
        "https://raw.githubusercontent.com/AimesSoft/erika_plugins/refs/heads/main/tools/icon.png"
    );
    assert!(
        candidates(base)
            .iter()
            .skip(1)
            .all(|url| url.ends_with(base))
    );
}
