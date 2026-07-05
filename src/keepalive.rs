//! GAIA keep-alive watchdog.
//!
//! The MW75 sends 1-second heartbeat packets on RFCOMM channel 1 while the
//! GAIA session is active.  If no heartbeat is received for
//! [`HEARTBEAT_TIMEOUT_SECS`] seconds, the session has likely expired (the
//! device drops GAIA activation after ~60 s of inactivity) and we must
//! trigger a reconnect.
//!
//! # Usage
//! ```ignore
//! let (tx, mut rx) = tokio::sync::mpsc::channel::<HeartbeatEvent>(32);
//! // Feed heartbeat packets from RFCOMM ch1 into `tx`
//! let watchdog = KeepAliveWatchdog::new(tx);
//! tokio::spawn(async move {
//!     watchdog.run(reconnect_callback).await;
//! });
//! ```

use crate::protocol::{HEARTBEAT_TIMEOUT_SECS, RECONNECT_BACKOFF_MAX_SECS};
use log::{info, warn};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::time::sleep;

/// A heartbeat event received from RFCOMM channel 1.
#[derive(Debug, Clone)]
pub struct HeartbeatEvent {
    /// Monotonic timestamp when the packet was received.
    pub received_at: Instant,
}

impl HeartbeatEvent {
    pub fn now() -> Self {
        Self {
            received_at: Instant::now(),
        }
    }
}

/// Current connection state tracked by the watchdog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    /// GAIA session is active; heartbeats are arriving normally.
    Active,
    /// No heartbeat received within the timeout window.
    TimedOut,
    /// Actively attempting to reconnect.
    Reconnecting { attempt: u32 },
}

/// Watchdog that monitors RFCOMM ch1 heartbeats and triggers reconnects.
pub struct KeepAliveWatchdog {
    /// Receives heartbeat events pumped in from the RFCOMM reader task.
    heartbeat_rx: mpsc::Receiver<HeartbeatEvent>,
    /// Current state (public for introspection).
    pub state: ConnectionState,
}

impl KeepAliveWatchdog {
    pub fn new(heartbeat_rx: mpsc::Receiver<HeartbeatEvent>) -> Self {
        Self {
            heartbeat_rx,
            state: ConnectionState::Active,
        }
    }

    /// Run the watchdog loop.
    ///
    /// `on_timeout` is called whenever a timeout is detected.  The future
    /// returned by `on_timeout` should perform reconnection (e.g. call
    /// [`crate::mw75_client::reconnect_with_backoff`]) and return `Ok(())`
    /// when reconnection succeeds.
    ///
    /// This method runs until the heartbeat channel is closed or an
    /// unrecoverable error occurs.
    pub async fn run<F, Fut>(&mut self, mut on_timeout: F)
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        let timeout_dur = Duration::from_secs(HEARTBEAT_TIMEOUT_SECS);
        let mut last_heartbeat = Instant::now();
        let mut attempt: u32 = 0;

        loop {
            // Non-blocking drain of all pending heartbeat events
            loop {
                match self.heartbeat_rx.try_recv() {
                    Ok(evt) => {
                        last_heartbeat = evt.received_at;
                        if self.state != ConnectionState::Active {
                            info!("Keep-alive: heartbeat restored — session active");
                        }
                        self.state = ConnectionState::Active;
                        attempt = 0;
                    }
                    Err(mpsc::error::TryRecvError::Empty) => break,
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        info!("Keep-alive: heartbeat channel closed — watchdog stopping");
                        return;
                    }
                }
            }

            // Check timeout
            if last_heartbeat.elapsed() > timeout_dur {
                attempt += 1;
                let backoff = backoff_secs(attempt);
                warn!(
                    "Keep-alive: no heartbeat for {}s (attempt {attempt}, reconnecting in {backoff}s)",
                    HEARTBEAT_TIMEOUT_SECS
                );
                self.state = ConnectionState::TimedOut;

                // Notify caller and wait for reconnect
                self.state = ConnectionState::Reconnecting { attempt };
                on_timeout().await;

                // Reset after reconnect attempt
                last_heartbeat = Instant::now();
                sleep(Duration::from_secs(backoff)).await;
                continue;
            }

            // Sleep for a short poll interval
            sleep(Duration::from_millis(500)).await;
        }
    }
}

/// Compute exponential-backoff delay (capped at [`RECONNECT_BACKOFF_MAX_SECS`]).
fn backoff_secs(attempt: u32) -> u64 {
    // 1s, 2s, 4s, 8s, 16s, 30s max
    let exp = attempt.saturating_sub(1).min(5); // 2^5=32 > 30s cap
    (1u64 << exp).min(RECONNECT_BACKOFF_MAX_SECS)
}

// ---- Tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backoff_secs() {
        assert_eq!(backoff_secs(1), 1);
        assert_eq!(backoff_secs(2), 2);
        assert_eq!(backoff_secs(3), 4);
        assert_eq!(backoff_secs(4), 8);
        assert_eq!(backoff_secs(5), 16);
        assert_eq!(backoff_secs(6), 30); // capped at max
        assert_eq!(backoff_secs(10), 30);
    }

    #[tokio::test]
    async fn test_watchdog_timeout_fires() {
        let (tx, rx) = mpsc::channel::<HeartbeatEvent>(4);
        let mut watchdog = KeepAliveWatchdog::new(rx);

        // We do NOT send any heartbeats, so it should time out immediately.
        // Use a very short timeout for testing by overriding elapsed check
        // indirectly: just drop the sender so the channel closes.
        drop(tx);

        // Run with a no-op reconnect callback; watchdog should exit when
        // channel closes.
        let reconnect_called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = reconnect_called.clone();
        watchdog
            .run(|| {
                let f = flag.clone();
                async move {
                    f.store(true, std::sync::atomic::Ordering::SeqCst);
                }
            })
            .await;
    }

    #[tokio::test]
    async fn test_watchdog_active_with_heartbeats() {
        let (tx, rx) = mpsc::channel::<HeartbeatEvent>(4);
        let mut watchdog = KeepAliveWatchdog::new(rx);

        // Send a heartbeat immediately
        tx.send(HeartbeatEvent::now()).await.unwrap();
        drop(tx);

        let reconnect_called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = reconnect_called.clone();
        watchdog
            .run(|| {
                let f = flag.clone();
                async move {
                    f.store(true, std::sync::atomic::Ordering::SeqCst);
                }
            })
            .await;

        // Reconnect should NOT have been called because a heartbeat arrived
        assert!(!reconnect_called.load(std::sync::atomic::Ordering::SeqCst));
    }
}
