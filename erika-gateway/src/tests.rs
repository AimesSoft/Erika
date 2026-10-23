//! The gateway before a stand-in for Misaka: a show it matches by name,
//! one whose ids it changed (re-imported), its search and its danmaku.

use super::*;
use axum::extract::Query;

const MD5: &str = "0123456789abcdef0123456789abcdef";

async fn misaka_match(axum::Json(request): axum::Json<Value>) -> axum::Json<Value> {
    let name = request
        .get("fileName")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if name.contains("S01E02") {
        return axum::Json(
            json!({ "success": true, "isMatched": true, "matches": [{ "episodeId": 25_000_136_010_002i64, "animeId": 136, "animeTitle": "测试剧", "episodeTitle": "第2集", "type": "tvseries", "typeDescription": "电视剧", "shift": 0 }] }),
        );
    }
    axum::Json(json!({ "success": true, "isMatched": false, "matches": [] }))
}

async fn misaka_bangumi(Path(id): Path<i64>) -> axum::Json<Value> {
    // Show 136 was imported again as 137; 136 is gone.
    if id == 137 {
        return axum::Json(
            json!({ "success": true, "bangumi": { "animeId": 137, "animeTitle": "测试剧", "episodes": [{ "episodeId": 25_000_137_010_002i64, "episodeTitle": "第2集", "episodeNumber": "2" }] } }),
        );
    }
    axum::Json(json!({ "success": false, "errorCode": 404, "bangumi": null }))
}

async fn misaka_search(Query(query): Query<HashMap<String, String>>) -> axum::Json<Value> {
    if query.get("anime").map(String::as_str) == Some("测试剧") {
        return axum::Json(json!({ "success": true, "animes": [
            { "animeId": 99, "animeTitle": "测试剧 第二季", "episodes": [{ "episodeId": 25_000_099_010_002i64, "episodeTitle": "x" }] },
            { "animeId": 137, "animeTitle": "测试剧", "imageUrl": "https://img/137.jpg", "episodes": [
                { "episodeId": 25_000_137_010_001i64, "episodeTitle": "第1集" },
                { "episodeId": 25_000_137_010_002i64, "episodeTitle": "第2集" }
            ] }
        ] }));
    }
    axum::Json(json!({ "success": true, "animes": [] }))
}

async fn misaka_anime_search(Query(query): Query<HashMap<String, String>>) -> axum::Json<Value> {
    let found = query
        .get("keyword")
        .is_some_and(|keyword| "测试剧".contains(keyword.as_str()));
    axum::Json(
        json!({ "success": true, "animes": if found { json!([{ "animeId": 137, "animeTitle": "测试剧", "type": "tvseries", "typeDescription": "电视剧", "episodeCount": 2 }]) } else { json!([]) } }),
    )
}

async fn misaka_comment(Path(id): Path<i64>) -> axum::Json<Value> {
    axum::Json(
        json!({ "count": 1, "comments": [{ "cid": 1, "p": "1.0,1,16777215,u", "m": format!("弹幕 {id}") }] }),
    )
}

/// The stand-in Misaka, at `/api/v1/misaka`, and a gateway before it.
async fn gateway() -> (String, Arc<Gateway>) {
    let misaka = Router::new()
        .route("/api/v1/misaka/api/v2/match", post(misaka_match))
        .route("/api/v1/misaka/api/v2/bangumi/{id}", get(misaka_bangumi))
        .route("/api/v1/misaka/api/v2/search/episodes", get(misaka_search))
        .route(
            "/api/v1/misaka/api/v2/search/anime",
            get(misaka_anime_search),
        )
        .route("/api/v1/misaka/api/v2/comment/{id}", get(misaka_comment));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream = format!("http://{}/api/v1/misaka", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, misaka).await.unwrap() });
    let gateway = Arc::new(Gateway::new("secret".into(), upstream, Store::in_memory()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let app = app(Arc::clone(&gateway));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (address, gateway)
}

async fn post_json(url: &str, body: Value) -> (u16, Value) {
    let answer = reqwest::Client::new()
        .post(url)
        .json(&body)
        .send()
        .await
        .unwrap();
    (
        answer.status().as_u16(),
        answer.json().await.unwrap_or(Value::Null),
    )
}

fn episode(answer: &Value) -> Option<i64> {
    answer
        .pointer("/matches/0/episodeId")
        .and_then(Value::as_i64)
}

#[tokio::test]
async fn a_confirmed_file_is_known_by_its_fingerprint_whatever_its_name() {
    let (address, gateway) = gateway().await;
    let api = format!("{address}/secret/api/v2");

    // Without the token, nothing.
    let (status, _) = post_json(
        &format!("{address}/wrong/api/v2/match"),
        json!({ "fileName": "x" }),
    )
    .await;
    assert_eq!(status, 401);

    // Unknown fingerprints go to Misaka, which matches by name.
    let (_, answer) = post_json(
        &format!("{api}/match"),
        json!({ "fileName": "测试剧.S01E02.mkv", "fileHash": MD5, "fileSize": 100 }),
    )
    .await;
    assert_eq!(episode(&answer), Some(25_000_136_010_002));
    let (_, answer) = post_json(
        &format!("{api}/match"),
        json!({ "fileName": "renamed.mkv", "fileHash": MD5, "fileSize": 100 }),
    )
    .await;
    assert_eq!(answer.get("isMatched"), Some(&json!(false)));

    // Confirmed, the file is known under any name. Misaka has since moved
    // the show to 137: the episode is found again by title and number.
    let confirmed = json!({ "fileHash": MD5, "fileSize": 100, "fileName": "测试剧.S01E02.mkv", "animeId": 136, "episodeId": 25_000_136_010_002i64, "animeTitle": "测试剧", "episodeTitle": "第2集", "typeDescription": "电视剧" });
    let (status, _) = post_json(&format!("{api}/match/confirm"), confirmed.clone()).await;
    assert_eq!(status, 200);
    let (_, answer) = post_json(
        &format!("{api}/match"),
        json!({ "fileName": "renamed.mkv", "fileHash": MD5.to_uppercase(), "fileSize": 100 }),
    )
    .await;
    assert_eq!(episode(&answer), Some(25_000_137_010_002));
    assert_eq!(
        answer.pointer("/matches/0/imageUrl"),
        Some(&json!("https://img/137.jpg"))
    );
    let kept = gateway.store.find(MD5, 100).unwrap();
    assert_eq!(
        (kept.anime_id, kept.episode_id, kept.number),
        (137, 25_000_137_010_002, 2)
    );

    // Another size is another file.
    let (_, answer) = post_json(
        &format!("{api}/match"),
        json!({ "fileName": "renamed.mkv", "fileHash": MD5, "fileSize": 101 }),
    )
    .await;
    assert_eq!(answer.get("isMatched"), Some(&json!(false)));

    // Danmaku and the rest pass through.
    let comments: Value = reqwest::get(format!("{api}/comment/25000137010002?withRelated=true"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        comments.pointer("/comments/0/m"),
        Some(&json!("弹幕 25000137010002"))
    );

    // Unlinked, the file is Misaka's to guess again.
    post_json(
        &format!("{api}/match/forget"),
        json!({ "fileHash": MD5, "fileSize": 100 }),
    )
    .await;
    let (_, answer) = post_json(
        &format!("{api}/match"),
        json!({ "fileName": "renamed.mkv", "fileHash": MD5, "fileSize": 100 }),
    )
    .await;
    assert_eq!(answer.get("isMatched"), Some(&json!(false)));
}

#[tokio::test]
async fn confirming_the_same_episode_again_counts_and_another_replaces_it() {
    let store = Store::in_memory();
    let binding = |episode_id: i64| Binding {
        anime_id: 1,
        episode_id,
        anime_title: "A".into(),
        episode_title: String::new(),
        number: episode_id % 10_000,
        kind: "tvseries".into(),
        kind_description: String::new(),
        image: String::new(),
        confirmations: 1,
    };
    store.confirm(MD5, 1, "a.mkv", &binding(10_001)).unwrap();
    store.confirm(MD5, 1, "a.mkv", &binding(10_001)).unwrap();
    assert_eq!(store.find(MD5, 1).unwrap().confirmations, 2);
    store.confirm(MD5, 1, "a.mkv", &binding(10_003)).unwrap();
    let kept = store.find(MD5, 1).unwrap();
    assert_eq!(
        (kept.episode_id, kept.number, kept.confirmations),
        (10_003, 3, 1)
    );
    assert_eq!(store.count(), 1);
}

/// Erika's own client before the gateway: what it is given carries the
/// server's ids negated, and it gives them back the same way.
#[tokio::test(flavor = "multi_thread")]
async fn erika_matches_confirms_and_reads_through_the_gateway() {
    use erika_dandanplay::custom;
    let (address, _gateway) = gateway().await;
    tokio::task::spawn_blocking(move || {
        custom::set_server(Some(format!("{address}/secret/")));
        let matched = custom::match_video("测试剧.S01E02.mkv", MD5, 100)
            .unwrap()
            .unwrap();
        assert_eq!(
            (matched.anime_id, matched.episode_id, matched.number()),
            (-136, -25_000_136_010_002, 2)
        );
        assert_eq!(custom::match_video("renamed.mkv", MD5, 100).unwrap(), None);
        custom::confirm(&custom::Confirmed {
            file_name: "测试剧.S01E02.mkv",
            md5: MD5,
            size: 100,
            anime_id: matched.anime_id,
            episode_id: matched.episode_id,
            number: matched.number(),
            anime_title: &matched.anime_title,
            episode_title: &matched.episode_title,
            image: "",
        })
        .unwrap();
        // Known now by its fingerprint, under Misaka's new ids.
        let again = custom::match_video("renamed.mkv", MD5, 100)
            .unwrap()
            .unwrap();
        assert_eq!(
            (again.anime_id, again.episode_id),
            (-137, -25_000_137_010_002)
        );
        assert_eq!(again.image_url, "https://img/137.jpg");
        // Details and danmaku of negative ids come from the server.
        let details = erika_dandanplay::details(-137).unwrap();
        assert_eq!(
            (details.anime_id, details.episodes[0].episode_id),
            (-137, -25_000_137_010_002)
        );
        let (json, count) = erika_dandanplay::comments(-25_000_137_010_002).unwrap();
        assert_eq!(count, 1);
        assert!(json.contains("弹幕 25000137010002"));
        assert_eq!(custom::search("测试").unwrap()[0].anime_id, -137);
        custom::forget(MD5, 100).unwrap();
        assert_eq!(custom::match_video("renamed.mkv", MD5, 100).unwrap(), None);
        // A wrong token says so.
        custom::set_server(Some(format!("{address}/wrong")));
        assert!(
            custom::match_video("x.mkv", MD5, 100)
                .unwrap_err()
                .message
                .contains("令牌")
        );
    })
    .await
    .unwrap();
}
