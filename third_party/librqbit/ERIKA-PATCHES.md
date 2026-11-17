# librqbit 8.1.1 in Erika

Source: crates.io `librqbit` 8.1.1, upstream repository
https://github.com/ikatson/rqbit, commit
`00b97485160ff5b5aa2b379ea0815d568ec665f0`, directory `crates/librqbit`.
Licensed under Apache-2.0; see [LICENSE](LICENSE), copied from that commit.
The Cargo-normalized release manifest is retained. Registry metadata, the
upstream lockfile and original pre-normalization manifest are not included.

## Fragmented BitTorrent handshakes

`src/read_buf.rs` previously parsed a complete handshake after a single TCP
read. TCP can legally deliver fewer than 68 bytes: this rejected valid peers,
including real 48+20 and 20+48 reads observed from Erika's Windows machine.

The patch collects fragments inside the original single timeout budget.
Invalid headers and EOF fail without extending that budget. Bytes following
the handshake in its final read remain buffered for `read_message`.
No peer identity, tracker, proxy, timeout value or retry policy is changed.

`src/read_buf_tests.rs` covers every split (1 through 67), bytewise delivery,
complete delivery, coalesced Unchoke messages, invalid headers, early EOF and
a single total deadline with Tokio virtual time. The `test-util` development
feature is enabled explicitly; it does not change production Tokio features.

Erika references this package directly by relative path so separately generated
native host workspaces use the same fix. Remove the vendored package only when
a pinned upstream version includes equivalent behavior and passes these tests.
Fixing rejected handshakes alone does not establish that any particular swarm
will provide pieces; actual progress must be measured separately.

## Keep-alive-aware read timeouts, slot isolation, and request permits

Three additional root causes blocked real downloads even after handshake
fragmentation was fixed:

1. `src/peer_connection.rs` defaulted both `connect_timeout` and
   `read_write_timeout` to 10 seconds and applied that 10-second timeout to
   every `read_message` call, including peers legitimately silent while choked
   between BEP 3 120-second keep-alives (observed on real seeders taking ~13s
   to unchoke), causing repeated disconnects and 6x exponential backoff.
   - Default `connect_timeout` is adjusted to 4s unless explicitly configured,
     and `PeerConnectionHandler::read_timeout` is consulted when
     `read_write_timeout` is `None`: peers holding needed pieces or expressing
     interest get `keep_alive_interval + 15s` (135s default); active in-flight
     chunk transfers get at least 30s; peers awaiting their first post-handshake
     message get at least 15s; post-handshake peers offering neither needed
     pieces nor interest are released after 2s instead of occupying slots.
2. `src/torrent_state/live/mod.rs` and `src/chunk_tracker.rs`:
   - `dial_semaphore` (128) is separated from `peer_semaphore` (128 live/incoming
     slots) so unreachable peers in queues do not starve live slots; local
     capacity rejections do not incur error counters or exponential backoff;
     port-0 peers are ignored.
   - `task_peer_chunk_requester` checks `ChunkTracker::has_any_needed_piece`
     before sending `Interested` or waiting for unchoke, and waits for re-unchoke
     mid-piece if choked.
   - `on_i_am_unchoked` caps `requests_sem` permits + `inflight_requests` at 128
     instead of adding 128 permits on every choke/unchoke transition.

Covered by unit tests in `src/chunk_tracker.rs`, `src/peer_connection.rs`, and
`src/torrent_state/live/live_tests.rs`.

