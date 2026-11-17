use super::ReadBuf;
use peer_binary_protocol::MessageBorrowed;
use std::{
    collections::VecDeque,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncWriteExt, ReadBuf as TokioReadBuf};

/// Return precisely one fragment per read, regardless of buffer capacity.
struct Segments(VecDeque<Vec<u8>>);

impl Segments {
    fn new(parts: impl IntoIterator<Item = Vec<u8>>) -> Self {
        Self(parts.into_iter().collect())
    }
}

impl AsyncRead for Segments {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        output: &mut TokioReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if let Some(part) = self.0.pop_front() {
            let count = part.len().min(output.remaining());
            output.put_slice(&part[..count]);
            if count < part.len() {
                self.0.push_front(part[count..].to_vec());
            }
        }
        Poll::Ready(Ok(()))
    }
}

fn handshake() -> Vec<u8> {
    let mut data = vec![19];
    data.extend_from_slice(b"BitTorrent protocol");
    data.extend_from_slice(&[0; 8]);
    data.extend_from_slice(&[42; 20]);
    data.extend_from_slice(b"-rQ5310-123456789012");
    assert_eq!(data.len(), 68);
    data
}

#[tokio::test]
async fn complete_handshake_is_accepted() {
    let mut buffer = ReadBuf::new();
    let received = buffer
        .read_handshake(Segments::new([handshake()]), Duration::from_secs(1))
        .await
        .unwrap();
    assert_eq!(received.info_hash, [42; 20]);
    assert_eq!(received.peer_id, *b"-rQ5310-123456789012");
}

#[tokio::test]
async fn fragmented_handshake_accepts_every_split() {
    let data = handshake();
    for split in 1..data.len() {
        let mut buffer = ReadBuf::new();
        let result = buffer
            .read_handshake(
                Segments::new([data[..split].to_vec(), data[split..].to_vec()]),
                Duration::from_secs(1),
            )
            .await;
        assert!(result.is_ok(), "TCP split at {split} failed: {result:?}");
    }
}

#[tokio::test]
async fn fragmented_handshake_accepts_one_byte_reads() {
    let mut buffer = ReadBuf::new();
    let received = buffer
        .read_handshake(
            Segments::new(handshake().into_iter().map(|byte| vec![byte])),
            Duration::from_secs(1),
        )
        .await
        .unwrap();
    assert_eq!(received.info_hash, [42; 20]);
}

#[tokio::test]
async fn fragmented_handshake_retains_coalesced_message() {
    let data = handshake();
    // Test both one read and the 48+20 split observed on real peers.
    for split in [0, 48] {
        let mut tail = data[split..].to_vec();
        tail.extend_from_slice(&[0, 0, 0, 1, 1]); // Unchoke
        let parts = if split == 0 {
            vec![tail]
        } else {
            vec![data[..split].to_vec(), tail]
        };
        let mut buffer = ReadBuf::new();
        buffer
            .read_handshake(Segments::new(parts), Duration::from_secs(1))
            .await
            .unwrap();
        // There are no more network bytes: this must come from the buffer.
        let message = buffer
            .read_message(tokio::io::empty(), Duration::from_secs(1))
            .await
            .unwrap();
        assert!(matches!(message, MessageBorrowed::Unchoke));
    }
}

#[tokio::test]
async fn fragmented_handshake_reports_early_eof() {
    let data = handshake();
    for end in [0, 1, 20, 48, 67] {
        let mut buffer = ReadBuf::new();
        let parts = if end == 0 {
            vec![]
        } else {
            vec![data[..end].to_vec()]
        };
        let error = buffer
            .read_handshake(Segments::new(parts), Duration::from_secs(1))
            .await
            .unwrap_err();
        assert!(
            format!("{error:#}").contains("peer disconnected while reading handshake"),
            "unexpected EOF error: {error:#}"
        );
    }
}

#[tokio::test]
async fn invalid_handshake_header_is_rejected_without_reading_more() {
    let mut stream = Segments::new([vec![0], handshake()]);
    let mut buffer = ReadBuf::new();
    let error = buffer
        .read_handshake(&mut stream, Duration::from_secs(1))
        .await
        .unwrap_err();
    assert!(format!("{error:#}").contains("pstr should be 19 bytes long"));
    assert_eq!(stream.0.len(), 1, "invalid header triggered another read");
}

#[tokio::test(start_paused = true)]
async fn fragmented_handshake_uses_one_total_deadline() {
    let data = handshake();
    let (mut writer, mut reader) = tokio::io::duplex(128);
    let peer = tokio::spawn(async move {
        writer.write_all(&data[..20]).await.unwrap();
        tokio::time::sleep(Duration::from_secs(6)).await;
        writer.write_all(&data[20..48]).await.unwrap();
        tokio::time::sleep(Duration::from_secs(6)).await;
        let _ = writer.write_all(&data[48..]).await;
    });
    let started = tokio::time::Instant::now();
    let mut buffer = ReadBuf::new();
    let error = buffer
        .read_handshake(&mut reader, Duration::from_secs(10))
        .await
        .unwrap_err();
    assert!(format!("{error:#}").contains("timeout"));
    assert_eq!(started.elapsed(), Duration::from_secs(10));
    peer.abort();
}
