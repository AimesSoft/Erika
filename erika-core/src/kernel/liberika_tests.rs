//! Exercise the real adapter/execute hand-off without starting FFmpeg or a GUI.
use super::*;
use std::sync::Weak;

#[test]
fn kernel_diagnostics_expose_categories_not_private_transport_details() {
    let private = "https://sentinel-user:sentinel-password@example.invalid/private-token/video?secret=sentinel-query\nAuthorization: Bearer sentinel-header\nCookie: session=sentinel-cookie\nserver response: arbitrary private data";
    let cases = [
        (PlayerError::Closed, "播放内核已关闭"),
        (
            PlayerError::InvalidStateTransition {
                from: PlayerState::Closed,
                to: PlayerState::Playing,
            },
            "当前播放状态不允许此操作",
        ),
        (PlayerError::Renderer(private.into()), "画面渲染失败"),
        (
            PlayerError::RendererBackpressure(private.into()),
            "画面输出暂时繁忙",
        ),
        (PlayerError::Source(private.into()), "无法读取媒体来源"),
        (PlayerError::Playback(private.into()), "媒体播放失败"),
    ];
    for (error, expected) in cases {
        assert_eq!(error_message(&error), expected);
        let reply = failed(error);
        assert_eq!(reply.code, "media_error");
        assert_eq!(reply.message, expected);
    }
}

struct Probe {
    kernel: Weak<Liberika>,
    addresses: Mutex<Vec<String>>,
    errors: Mutex<Vec<String>>,
    accept: bool,
}
impl KernelHub for Probe {
    fn dispatch(
        &self,
        command: MediaCommand,
        params: Vec<Value>,
        _reply: Box<dyn FnOnce(Result<Value, CommandError>) + Send>,
    ) {
        if command == MediaCommand::Open {
            let Value::Record(fields) = &params[1] else {
                panic!("media record")
            };
            let Value::Text(url) = &fields[0] else {
                panic!("media address")
            };
            self.addresses.lock().unwrap().push(url.clone());
            if self.accept {
                self.kernel.upgrade().unwrap().execute(request(url, PLAYER));
            }
        }
    }
    fn complete(&self, _request: u64, result: Result<(), CommandError>) {
        if let Err(error) = result {
            self.errors.lock().unwrap().push(error.code);
        }
    }
    fn observe(&self, _player: &str, _version: u64, _observation: Observation) {}
}
fn request(url: &str, player: &str) -> ExecRequest {
    ExecRequest {
        request: 1,
        player: player.into(),
        source_version: 1,
        op: ExecOp::Load { url: url.into() },
    }
}
fn adapter(
    accept: bool,
) -> (
    Arc<Liberika>,
    Arc<Probe>,
    std::sync::mpsc::Receiver<Message>,
) {
    let (sender, receiver) = std::sync::mpsc::channel();
    let kernel = Arc::new(Liberika {
        sender,
        hub: Arc::new(Mutex::new(None)),
        playback: Arc::default(),
        results: Arc::default(),
        frames: Arc::default(),
        opens: Mutex::default(),
    });
    let hub = Arc::new(Probe {
        kernel: Arc::downgrade(&kernel),
        addresses: Mutex::default(),
        errors: Mutex::default(),
        accept,
    });
    kernel.attached(hub.clone());
    (kernel, hub, receiver)
}

#[test]
fn private_open_packets_are_one_use_and_do_not_share_headers_by_url() {
    let (a, hub, messages) = adapter(true);
    let url = "https://example.invalid/video?token=test-private";
    for (start, token) in [(10., "first"), (20., "second")] {
        a.open_with_headers(url, start, vec![("Authorization".into(), token.into())])
            .unwrap();
    }
    for (start, token) in [(10., "first"), (20., "second")] {
        let Message::Request(request, Some(opening)) = messages.try_recv().unwrap() else {
            panic!("private packet")
        };
        let ExecOp::Load { url: actual } = request.op else {
            panic!("load")
        };
        assert_eq!(actual, url);
        assert_eq!(opening.start, start);
        assert_eq!(opening.headers, [("Authorization".into(), token.into())]);
    }
    assert!(a.opens.lock().unwrap().is_empty());
    let addresses = hub.addresses.lock().unwrap().clone();
    assert_ne!(addresses[0], addresses[1]);
    assert!(
        addresses
            .iter()
            .all(|s| s.starts_with("erika-playback://") && !s.contains("test-private"))
    );
    a.execute(request(&addresses[0], PLAYER));
    assert!(messages.try_recv().is_err());
    assert_eq!(hub.errors.lock().unwrap().as_slice(), &["expired_open"]);
    let (b, other, received) = adapter(true);
    b.execute(request(&addresses[1], PLAYER));
    assert!(received.try_recv().is_err());
    assert_eq!(other.errors.lock().unwrap().as_slice(), &["expired_open"]);
}

#[test]
fn rejected_dispatch_drops_its_private_packet_and_wrong_player_cannot_take_it() {
    let (kernel, hub, messages) = adapter(false);
    assert!(
        kernel
            .open_with_headers(
                "https://example.invalid/",
                0.,
                vec![("Authorization".into(), "private".into())]
            )
            .is_err()
    );
    assert!(kernel.opens.lock().unwrap().is_empty());
    assert!(messages.try_recv().is_err());
    let handle = "erika-playback://test-only";
    kernel.opens.lock().unwrap().insert(
        handle.into(),
        Opening {
            url: "https://example.invalid/".into(),
            start: 3.,
            headers: vec![],
        },
    );
    kernel.execute(request(handle, "another-player"));
    assert!(messages.try_recv().is_err());
    assert!(kernel.opens.lock().unwrap().contains_key(handle));
    assert_eq!(hub.errors.lock().unwrap().as_slice(), &["expired_open"]);
    kernel.execute(request(handle, PLAYER));
    assert!(matches!(
        messages.try_recv().unwrap(),
        Message::Request(_, Some(_))
    ));
    assert!(kernel.opens.lock().unwrap().is_empty());
}
