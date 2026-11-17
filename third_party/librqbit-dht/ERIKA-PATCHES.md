# librqbit-dht 5.3.1 in Erika

Source: crates.io `librqbit-dht` 5.3.1, upstream repository
https://github.com/ikatson/rqbit, commit
`00b97485160ff5b5aa2b379ea0815d568ec665f0`, directory `crates/librqbit_dht`.
Licensed under Apache-2.0; see [LICENSE](LICENSE), copied from that commit.

## Windows UDP ICMP reset resilience

`src/dht.rs` in `DhtWorker::framer` previously exited with a fatal error on
any `UdpSocket::recv_from` error. On Windows Winsock, an ICMP Port/Time
Exceeded reply to any earlier outbound DHT query causes the next `recv_from`
on the connectionless UDP socket to fail with `WSAENETRESET` (`10052`) or
`WSAECONNRESET` (`10054`), killing the DHT worker within ~1 second of startup.

The patch treats transient per-datagram UDP errors (`ConnectionReset`,
`ConnectionAborted`, `Interrupted`, `WouldBlock`, and Winsock `10040`/`10052`/`10054`)
as non-fatal so the DHT socket stays alive.

Remove this vendored package only when a pinned upstream version handles these
errors cleanly and passes the Windows DHT persistence check.
