//! Cancellation boundary for persistent HTTP read-ahead streams.
//!
//! Foreground and persistent background reads use reqwest on one shared Tokio
//! runtime. Cancellation drops the request future and interrupts connect, TLS,
//! response headers, and response body waits. System DNS resolution already
//! running in a blocking resolver may finish separately.

use std::future::Future;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use tokio::runtime::{Builder, Runtime};
use tokio_util::sync::CancellationToken;

use super::{Result, SourceError};

#[derive(Clone, Debug)]
pub struct SourceCancellation(Arc<Mutex<CancellationState>>);

#[derive(Debug)]
struct CancellationState {
    token: CancellationToken,
    closed: bool,
    generation: u64,
}

impl Default for SourceCancellation {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(CancellationState {
            token: CancellationToken::new(),
            closed: false,
            generation: 0,
        })))
    }
}

impl SourceCancellation {
    pub fn cancel(&self) {
        let mut state = self.0.lock().expect("source cancellation mutex poisoned");
        state.closed = true;
        state.token.cancel();
    }

    pub(crate) fn interrupt(&self) -> u64 {
        let mut state = self.0.lock().expect("source cancellation mutex poisoned");
        state.token.cancel();
        state.generation = state.generation.wrapping_add(1);
        state.generation
    }

    pub(crate) fn resume(&self, generation: u64) {
        let mut state = self.0.lock().expect("source cancellation mutex poisoned");
        if !state.closed && state.generation == generation && state.token.is_cancelled() {
            state.token = CancellationToken::new();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.token().is_cancelled()
    }

    fn child(&self) -> Self {
        Self(Arc::new(Mutex::new(CancellationState {
            token: self.token().child_token(),
            closed: false,
            generation: 0,
        })))
    }

    fn token(&self) -> CancellationToken {
        self.0
            .lock()
            .expect("source cancellation mutex poisoned")
            .token
            .clone()
    }
}

#[derive(Clone)]
pub(super) struct HttpIo {
    pub(super) client: reqwest::Client,
    pub(super) cancellation: SourceCancellation,
}

impl HttpIo {
    pub(super) fn new() -> Self {
        Self {
            client: shared_client().clone(),
            cancellation: SourceCancellation::default(),
        }
    }

    pub(super) fn child(&self) -> Self {
        Self {
            client: self.client.clone(),
            cancellation: self.cancellation.child(),
        }
    }

    pub(super) fn client(&self) -> &reqwest::Client {
        &self.client
    }

    pub(super) fn cancel(&self) {
        self.cancellation.cancel();
    }

    pub(super) fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }

    pub(super) fn run<T: Send + 'static>(
        &self,
        future: impl Future<Output = Result<T>> + Send + 'static,
    ) -> Result<T> {
        let cancellation = self.cancellation.token();
        let (sender, receiver) = crossbeam_channel::bounded(1);
        runtime().spawn(async move {
            // Keep the future in a narrower scope so its response, socket, and
            // partial buffers are dropped before completion is published to
            // the synchronous caller.
            let result = {
                tokio::pin!(future);
                tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => Err(SourceError::Cancelled),
                    result = &mut future => result,
                }
            };
            let _ = sender.send(result);
        });
        receiver
            .recv()
            .unwrap_or_else(|_| Err(SourceError::Http("HTTP worker stopped".to_string())))
    }

    pub(super) fn wait_cancelled(&self, duration: Duration) -> bool {
        self.run(async move {
            tokio::time::sleep(duration).await;
            Ok(())
        })
        .is_err()
    }
}

fn shared_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("build HTTP client")
    })
}

fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("erika-http-io")
            .enable_all()
            .build()
            .expect("create HTTP I/O runtime")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_interrupt_cannot_resume_newer_or_terminal_cancellation() {
        let cancellation = SourceCancellation::default();
        let first = cancellation.interrupt();
        let latest = cancellation.interrupt();
        cancellation.resume(first);
        assert!(cancellation.is_cancelled());
        cancellation.resume(latest);
        assert!(!cancellation.is_cancelled());
        cancellation.cancel();
        cancellation.resume(latest);
        assert!(cancellation.is_cancelled());
    }

    #[test]
    fn cancellation_drops_operation_resources_before_returning() {
        struct Resource(std::sync::mpsc::Sender<()>);
        impl Drop for Resource {
            fn drop(&mut self) {
                let _ = self.0.send(());
            }
        }

        let io = HttpIo::new();
        let cancellation = io.cancellation.clone();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (dropped_tx, dropped_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            io.run(async move {
                let _resource = Resource(dropped_tx);
                started_tx.send(()).unwrap();
                std::future::pending::<Result<()>>().await
            })
        });
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        cancellation.cancel();
        assert!(matches!(
            worker.join().unwrap(),
            Err(SourceError::Cancelled)
        ));
        assert!(dropped_rx.try_recv().is_ok());
    }
}
