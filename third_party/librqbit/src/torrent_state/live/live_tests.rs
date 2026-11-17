use super::*;
use crate::{
    create_torrent, tests::test_util::create_default_random_dir_with_torrents, AddTorrent,
    AddTorrentOptions, Session, SessionOptions,
};

async fn create_test_live_state() -> (
    Arc<Session>,
    Arc<TorrentStateLive>,
    tempfile::TempDir,
    tempfile::TempDir,
) {
    let src_dir = create_default_random_dir_with_torrents(1, 64 * 1024, Some("rqbit_live_test"));
    let out_dir = tempfile::TempDir::new().unwrap();
    let torrent = create_torrent(
        src_dir.path(),
        crate::CreateTorrentOptions {
            piece_length: Some(16384),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let session = Session::new_with_opts(
        out_dir.path().to_path_buf(),
        SessionOptions {
            disable_dht: true,
            disable_dht_persistence: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let handle = session
        .add_torrent(
            AddTorrent::TorrentFileBytes(torrent.as_bytes().unwrap()),
            Some(AddTorrentOptions {
                overwrite: true,
                output_folder: Some(out_dir.path().to_string_lossy().into_owned()),
                ..Default::default()
            }),
        )
        .await
        .unwrap()
        .into_handle()
        .unwrap();
    let live = loop {
        if let Some(live) = handle.live() {
            break live;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    (session, live, src_dir, out_dir)
}

fn make_test_handler(state: &Arc<TorrentStateLive>, addr: SocketAddr) -> PeerHandler {
    let (_rx, tx) = state.peers.mark_peer_connecting(addr).unwrap();
    let counters = state
        .peers
        .with_peer(addr, |p| p.stats.counters.clone())
        .unwrap();
    PeerHandler {
        state: state.clone(),
        counters,
        locked: RwLock::new(PeerHandlerLocked {
            i_am_choked: true,
            dial_permit: None,
            live_permit: None,
        }),
        on_bitfield_notify: Default::default(),
        unchoke_notify: Default::default(),
        requests_sem: Semaphore::new(0),
        addr,
        incoming: false,
        tx,
        first_message_received: AtomicBool::new(false),
    }
}

#[tokio::test]
async fn add_peer_ignores_port_zero() {
    let (_session, live, _src_dir, _out_dir) = create_test_live_state().await;
    assert!(!live
        .add_peer_if_not_seen("127.0.0.1:0".parse().unwrap())
        .unwrap());
    assert!(!live
        .peers
        .states
        .contains_key(&"127.0.0.1:0".parse().unwrap()));
}

#[tokio::test]
async fn read_timeout_adapts_by_peer_state() {
    let (_session, live, _src_dir, _out_dir) = create_test_live_state().await;
    let addr: SocketAddr = "127.0.0.2:6881".parse().unwrap();
    assert!(live.peers.add_if_not_seen(addr).is_some());
    let handler = make_test_handler(&live, addr);
    (&handler)
        .on_handshake(Handshake::new(live.info_hash(), live.peer_id()))
        .unwrap();

    let default_rw = Duration::from_secs(10);
    let keepalive = Duration::from_secs(120);

    // Before the first post-handshake message arrives, allow slow seeders 15s.
    assert_eq!(
        (&handler).read_timeout(default_rw, keepalive),
        Duration::from_secs(15)
    );

    // After a post-handshake message arrives without any needed pieces or interest, release in 2s.
    handler
        .first_message_received
        .store(true, Ordering::Release);
    assert_eq!(
        (&handler).read_timeout(default_rw, keepalive),
        Duration::from_secs(2)
    );

    // Once the peer announces needed pieces, wait at least keep_alive_interval + 15s for unchoke.
    let mut bf = vec![0u8; live.lengths.piece_bitfield_bytes()];
    bf[0] = 0b1000_0000;
    handler.on_bitfield(ByteBufOwned::from(bf)).unwrap();
    assert!((&handler).read_timeout(default_rw, keepalive) >= Duration::from_secs(135));

    // When a piece request is actively in flight, tolerate slow chunk responses (30s).
    let chunk = live
        .lengths
        .iter_chunk_infos(live.lengths.validate_piece_index(0).unwrap())
        .next()
        .unwrap();
    live.peers
        .with_live_mut(addr, "insert chunk", |l| l.inflight_requests.insert(chunk));
    assert_eq!(
        (&handler).read_timeout(default_rw, keepalive),
        Duration::from_secs(30)
    );
}

#[tokio::test]
async fn unchoke_caps_request_permits_at_128() {
    let (_session, live, _src_dir, _out_dir) = create_test_live_state().await;
    let addr: SocketAddr = "127.0.0.3:6881".parse().unwrap();
    assert!(live.peers.add_if_not_seen(addr).is_some());
    let handler = make_test_handler(&live, addr);
    (&handler)
        .on_handshake(Handshake::new(live.info_hash(), live.peer_id()))
        .unwrap();

    handler.on_i_am_unchoked();
    assert_eq!(handler.requests_sem.available_permits(), 128);

    handler.on_i_am_choked();
    handler.on_i_am_unchoked();
    handler.on_i_am_choked();
    handler.on_i_am_unchoked();
    assert_eq!(handler.requests_sem.available_permits(), 128);

    // Even with in-flight chunks already counted, total permits + inflight never exceed 128.
    let permit = handler.requests_sem.acquire().await.unwrap();
    permit.forget();
    let chunk = live
        .lengths
        .iter_chunk_infos(live.lengths.validate_piece_index(0).unwrap())
        .next()
        .unwrap();
    live.peers
        .with_live_mut(addr, "insert chunk", |l| l.inflight_requests.insert(chunk));
    handler.on_i_am_unchoked();
    assert_eq!(handler.requests_sem.available_permits(), 127);
}
