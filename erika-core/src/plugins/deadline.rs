//! A cancellable wake-up. The engine holds only a Send cancellation handle;
//! browser timer closures stay on their local executor and are dropped on cancel.
use std::time::Duration;

pub(super) struct Deadline {
    #[cfg(not(target_arch = "wasm32"))]
    _cancel: std::sync::mpsc::Sender<()>,
    #[cfg(target_arch = "wasm32")]
    _cancel: futures_channel::oneshot::Sender<()>,
}

impl Deadline {
    pub fn after(delay: Duration, fire: impl FnOnce() + Send + 'static) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (send, receive) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                if receive.recv_timeout(delay) == Err(std::sync::mpsc::RecvTimeoutError::Timeout) {
                    fire();
                }
            });
            Self { _cancel: send }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let (send, receive) = futures_channel::oneshot::channel::<()>();
            wasm_bindgen_futures::spawn_local(async move {
                let fired = futures_lite::future::or(
                    async {
                        let _ = receive.await;
                        false
                    },
                    async {
                        gloo_timers::future::TimeoutFuture::new(
                            delay.as_millis().min(u32::MAX as u128) as u32,
                        )
                        .await;
                        true
                    },
                )
                .await;
                if fired {
                    fire();
                }
            });
            Self { _cancel: send }
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn fires_once_and_drop_cancels_without_waiting_for_the_deadline() {
        fn is_send<T: Send>() {}
        is_send::<Deadline>();
        let (tx, rx) = std::sync::mpsc::channel();
        let timer = Deadline::after(Duration::from_millis(5), move || {
            tx.send(()).unwrap();
        });
        rx.recv_timeout(Duration::from_secs(1)).unwrap();
        drop(timer);
        let (tx, rx) = std::sync::mpsc::channel();
        let timer = Deadline::after(Duration::from_secs(60), move || {
            let _ = tx.send(());
        });
        drop(timer);
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(1)),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
        );
    }
}
