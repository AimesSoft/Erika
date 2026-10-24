//! The gateway before a stand-in for Misaka: a show it matches by name,
//! one whose ids it changed (re-imported), its search and its danmaku. Like
//! Misaka it names what it matched or found by passing ids (900000 and
//! up), takes its own ids as `A<id>` and lists only the episodes whose
//! danmaku it fetched. The owner's token is `secret`, the public key
//! `public`.

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
            json!({ "success": true, "isMatched": true, "matches": [{ "episodeId": 25_000_136_010_002i64, "animeId": 900_003, "animeTitle": "测试剧", "episodeTitle": "第2集", "type": "tvseries", "typeDescription": "电视剧", "shift": 0 }] }),
        );
    }
    axum::Json(json!({ "success": true, "isMatched": false, "matches": [] }))
}

async fn misaka_bangumi(Path(id): Path<String>) -> axum::Json<Value> {
    // Show 136 was imported again as 137, whose second episode's danmaku
    // were fetched; 136 is gone. 900001 is 137 as a search found it.
    match id.as_str() {
        "A137" => axum::Json(
            json!({ "success": true, "bangumi": { "animeId": 137, "animeTitle": "测试剧", "type": "tvseries", "typeDescription": "电视剧", "episodes": [{ "episodeId": 25_000_137_010_002i64, "episodeTitle": "第2集", "episodeNumber": "2" }] } }),
        ),
        "A900001" => axum::Json(
            json!({ "success": true, "bangumi": { "animeId": 900_001, "animeTitle": "测试剧 （来源：tencent）", "imageUrl": "https://img/137.jpg", "type": "other", "typeDescription": "其他", "episodes": [
                { "episodeId": 25_000_137_010_001i64, "episodeTitle": "第1集", "episodeNumber": "1" },
                { "episodeId": 25_000_137_010_002i64, "episodeTitle": "第2集", "episodeNumber": "2" }
            ] } }),
        ),
        _ => axum::Json(
            json!({ "success": true, "bangumi": null, "errorMessage": format!("找不到与标识符 '{id}' 关联的作品。") }),
        ),
    }
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
    gateway_allowing(120).await
}

/// The same, letting a device make `per_minute` requests a minute.
async fn gateway_allowing(per_minute: u32) -> (String, Arc<Gateway>) {
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
    let mut gateway = Gateway::new("secret".into(), upstream, Store::in_memory());
    gateway.public_key = Some("public".into());
    gateway.per_minute = per_minute;
    let gateway = Arc::new(gateway);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let app = app(Arc::clone(&gateway));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (address, gateway)
}

async fn post_json(url: &str, body: Value) -> (u16, Value) {
    post_as(url, None, body).await
}

/// A request from the device `device`.
async fn post_as(url: &str, device: Option<&str>, body: Value) -> (u16, Value) {
    let mut request = reqwest::Client::new().post(url).json(&body);
    if let Some(device) = device {
        request = request.header("X-Erika-Device", device);
    }
    let answer = request.send().await.unwrap();
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
    let kept = gateway.store.find(MD5, 100, "owner").unwrap();
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

fn binding(episode_id: i64) -> Binding {
    Binding {
        anime_id: 1,
        episode_id,
        anime_title: "A".into(),
        episode_title: String::new(),
        number: episode_id % 10_000,
        kind: "tvseries".into(),
        kind_description: String::new(),
        image: String::new(),
        confirmations: 1,
    }
}

fn device(name: &str) -> Voter {
    Voter {
        device: name.into(),
        trusted: false,
    }
}

#[test]
fn each_device_keeps_its_own_answer_and_others_hear_the_most_agreed() {
    let store = Store::in_memory();
    let owner = Voter {
        device: "owner".into(),
        trusted: true,
    };
    store.confirm(MD5, 1, &device("a"), "a.mkv", &binding(10_001)).unwrap();
    store.confirm(MD5, 1, &device("a"), "a.mkv", &binding(10_001)).unwrap();
    store.confirm(MD5, 1, &device("b"), "a.mkv", &binding(10_001)).unwrap();
    store.confirm(MD5, 1, &device("c"), "a.mkv", &binding(10_003)).unwrap();
    // Confirming again is one vote; c keeps its own answer.
    let heard = |who: &str| {
        let found = store.find(MD5, 1, who).unwrap();
        (found.episode_id, found.number, found.confirmations)
    };
    assert_eq!(heard("a"), (10_001, 1, 2));
    assert_eq!(heard("c"), (10_003, 3, 1));
    assert_eq!(heard("d"), (10_001, 1, 2));
    // The owner's word goes above any number of devices.
    store.confirm(MD5, 1, &owner, "a.mkv", &binding(10_003)).unwrap();
    assert_eq!(heard("d"), (10_003, 3, 2));
    assert_eq!(heard("a"), (10_001, 1, 2));
    assert_eq!(store.count(), 1);
    // A device unlinking takes its own vote back; the owner's takes all.
    store.forget(MD5, 1, &device("c")).unwrap();
    assert_eq!(heard("c"), (10_003, 3, 1));
    store.forget(MD5, 1, &owner).unwrap();
    assert_eq!(store.find(MD5, 1, "d"), None);
    assert_eq!(store.count(), 0);
}

#[tokio::test]
async fn shows_are_given_by_their_lasting_ids_with_every_episode_seen() {
    let (address, _gateway) = gateway().await;
    let api = format!("{address}/public/api/v2");
    let details = |id: &'static str| {
        let url = format!("{api}/bangumi/{id}");
        async move { reqwest::get(url).await.unwrap().json::<Value>().await.unwrap() }
    };
    let titles = |answer: &Value| -> Vec<String> {
        answer
            .pointer("/bangumi/episodes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|episode| episode.get("episodeTitle").and_then(Value::as_str).map(str::to_string))
            .collect()
    };
    // Before any search, Misaka lists only the episode it has danmaku of.
    assert_eq!(titles(&details("137").await), ["第2集"]);
    // A show a search found comes under the id its episodes carry.
    let found = details("900001").await;
    assert_eq!(found.pointer("/bangumi/animeId"), Some(&json!(137)));
    assert_eq!(found.pointer("/bangumi/animeTitle"), Some(&json!("测试剧")));
    assert_eq!(titles(&found), ["第1集", "第2集"]);
    // Asked again by that id, every episode seen is there.
    let again = details("A137").await;
    assert_eq!(titles(&again), ["第1集", "第2集"]);
    assert_eq!(again.pointer("/bangumi/imageUrl"), Some(&json!("https://img/137.jpg")));
    assert_eq!(again.pointer("/bangumi/type"), Some(&json!("tvseries")));
    // A show Misaka does not have stays unknown.
    assert_eq!(details("136").await.get("bangumi"), Some(&Value::Null));
}

#[tokio::test]
async fn under_the_public_key_devices_vote_for_themselves_at_a_rate() {
    let (address, _gateway) = gateway_allowing(4).await;
    let api = format!("{address}/public/api/v2");
    let confirmed = json!({ "fileHash": MD5, "fileSize": 100, "fileName": "测试剧.S01E02.mkv", "animeId": 137, "episodeId": 25_000_137_010_002i64, "episodeNumber": 2, "animeTitle": "测试剧", "episodeTitle": "第2集" });
    let (status, _) = post_as(&format!("{api}/match/confirm"), Some("a"), confirmed).await;
    assert_eq!(status, 200);
    // Another device hears what a confirmed; b's unlinking takes nothing.
    let renamed = json!({ "fileName": "renamed.mkv", "fileHash": MD5, "fileSize": 100 });
    let (_, answer) = post_as(&format!("{api}/match"), Some("b"), renamed.clone()).await;
    assert_eq!(episode(&answer), Some(25_000_137_010_002));
    post_as(&format!("{api}/match/forget"), Some("b"), json!({ "fileHash": MD5, "fileSize": 100 })).await;
    let (_, answer) = post_as(&format!("{api}/match"), Some("b"), renamed.clone()).await;
    assert_eq!(episode(&answer), Some(25_000_137_010_002));
    // b has made three requests this minute: the fourth is let in, the
    // fifth refused; a's are not.
    let (status, _) = post_as(&format!("{api}/match"), Some("b"), renamed.clone()).await;
    assert_eq!(status, 200);
    let (status, _) = post_as(&format!("{api}/match"), Some("b"), renamed.clone()).await;
    assert_eq!(status, 429);
    let (status, _) = post_as(&format!("{api}/match"), Some("a"), renamed.clone()).await;
    assert_eq!(status, 200);
    // Naming itself anew each time, a device is held to its address's rate:
    // ten devices' worth, of which a and b made seven.
    let mut statuses = Vec::new();
    for n in 0..34 {
        let (status, _) = post_as(&format!("{api}/match"), Some(&format!("new-{n}")), renamed.clone()).await;
        statuses.push(status);
    }
    assert_eq!(statuses.iter().filter(|status| **status == 200).count(), 33);
    assert_eq!(statuses.last(), Some(&429));
    // The owner is held to no rate.
    for _ in 0..6 {
        let (status, _) = post_json(&format!("{address}/secret/api/v2/match"), renamed.clone()).await;
        assert_eq!(status, 200);
    }
}

/// Erika's own client before the gateway: what it is given carries the
/// server's ids negated, and it gives them back the same way.
#[tokio::test(flavor = "multi_thread")]
async fn erika_matches_confirms_and_reads_through_the_gateway() {
    use erika_dandanplay::custom;
    let (address, _gateway) = gateway().await;
    tokio::task::spawn_blocking(move || {
        custom::set_server(Some(format!("{address}/public/")));
        custom::set_device("erika-test");
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
        // A wrong key says so.
        custom::set_server(Some(format!("{address}/wrong")));
        assert!(
            custom::match_video("x.mkv", MD5, 100)
                .unwrap_err()
                .message
                .contains("密钥")
        );
    })
    .await
    .unwrap();
}
