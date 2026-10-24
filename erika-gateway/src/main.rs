//! The gateway as a program, configured by its environment:
//!
//! - `ERIKA_GATEWAY_TOKEN`: the owner's token, in the path (required);
//! - `ERIKA_GATEWAY_PUBLIC_KEY`: the key built into Erika, which lets any
//!   device in at a limited rate (optional: without it only the owner);
//! - `ERIKA_GATEWAY_PER_MINUTE`: that rate, 120 requests a minute a device
//!   by default;
//! - `MISAKA_URL`: the danmaku server's dandanplay-style root, such as
//!   `http://127.0.0.1:7768/api/v1/<its token>` (required);
//! - `ERIKA_GATEWAY_LISTEN`: where to listen, `127.0.0.1:7769` by default;
//! - `ERIKA_GATEWAY_DB`: the database file, `erika-gateway.db` by default.

use erika_gateway::{Gateway, app, store::Store};
use std::sync::Arc;

fn required(name: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            eprintln!("erika-gateway: set {name}");
            std::process::exit(2);
        })
}

#[tokio::main]
async fn main() {
    let token = required("ERIKA_GATEWAY_TOKEN");
    let upstream = required("MISAKA_URL");
    let listen = std::env::var("ERIKA_GATEWAY_LISTEN").unwrap_or_else(|_| "127.0.0.1:7769".into());
    let path = std::env::var("ERIKA_GATEWAY_DB").unwrap_or_else(|_| "erika-gateway.db".into());
    let store = Store::open(std::path::Path::new(&path)).unwrap_or_else(|error| {
        eprintln!("erika-gateway: open {path}: {error}");
        std::process::exit(1);
    });
    eprintln!(
        "erika-gateway: {} files confirmed in {path}, listening on {listen}",
        store.count()
    );
    let mut gateway = Gateway::new(token, upstream, store);
    gateway.public_key = std::env::var("ERIKA_GATEWAY_PUBLIC_KEY").ok().filter(|key| !key.trim().is_empty());
    if let Some(per_minute) = std::env::var("ERIKA_GATEWAY_PER_MINUTE").ok().and_then(|value| value.parse().ok()) {
        gateway.per_minute = per_minute;
    }
    let gateway = Arc::new(gateway);
    let listener = tokio::net::TcpListener::bind(&listen)
        .await
        .unwrap_or_else(|error| {
            eprintln!("erika-gateway: listen on {listen}: {error}");
            std::process::exit(1);
        });
    axum::serve(listener, app(gateway))
        .with_graceful_shutdown(async {
            // Ctrl-C by hand, SIGTERM from systemd.
            #[cfg(unix)]
            {
                let mut term =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .expect("SIGTERM is watched");
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                }
            }
            #[cfg(not(unix))]
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .expect("the gateway serves");
}
