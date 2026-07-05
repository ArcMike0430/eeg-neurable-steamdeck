# GAIA Keep-Alive Watchdog

## Why a Keep-Alive is Needed

The MW75 Neuro drops its GAIA session after approximately **60 seconds** of BLE
inactivity.  When this happens, RFCOMM channel 25 stops delivering EEG frames
and the device must be re-activated (ENABLE_EEG + ENABLE_RAW_MODE).

The device signals session liveness by sending a **1-second heartbeat pulse**
on RFCOMM channel 1.  When the session drops, these heartbeats stop.

---

## Watchdog Architecture

```
RFCOMM ch1 reader ──heartbeat packets──▶ mpsc::channel ──▶ KeepAliveWatchdog
                                                                    │
                                                          last_heartbeat timer
                                                                    │
                                                          ┌─────────▼─────────┐
                                                          │ timeout? (5s)     │
                                                          └─────────┬─────────┘
                                                                    │ yes
                                                          ┌─────────▼─────────┐
                                                          │ on_timeout()       │
                                                          │  → reconnect_with_ │
                                                          │    backoff()       │
                                                          └───────────────────┘
```

---

## Reconnect Backoff Schedule

| Attempt | Delay |
|---|---|
| 1 | 1 s |
| 2 | 2 s |
| 3 | 4 s |
| 4 | 8 s |
| 5 | 16 s |
| 6+ | 30 s (max) |

---

## Configuration

Watchdog behaviour is governed by constants in `src/protocol.rs`:

```rust
pub const HEARTBEAT_TIMEOUT_SECS: u64 = 5;      // flag timeout after 5s silence
pub const GAIA_SESSION_TIMEOUT_SECS: u64 = 60;  // informational
pub const RECONNECT_BACKOFF_MAX_SECS: u64 = 30; // max reconnect delay
```

---

## Integration

The watchdog is wired into the main acquisition loop in `src/bin/eeg_stream.rs`:

```rust
// heartbeat channel: RfcommReader → KeepAliveWatchdog
let (hb_tx, hb_rx) = mpsc::channel::<HeartbeatEvent>(64);

// RfcommReader classifies incoming packets and forwards heartbeats via hb_tx
let (ring, _task) = RfcommReader::open(addr, eeg_channel, hb_tx).await?;

// Watchdog runs in a background task
let mut watchdog = KeepAliveWatchdog::new(hb_rx);
tokio::spawn(async move {
    watchdog.run(|| async { reconnect_with_backoff(adapter_idx).await }).await;
});
```

---

## Logging

The watchdog emits the following log messages:

| Event | Level | Message |
|---|---|---|
| Heartbeat restored | INFO | `Keep-alive: heartbeat restored — session active` |
| Timeout detected | WARN | `Keep-alive: no heartbeat for 5s (attempt N, reconnecting in Xs)` |
| Reconnect OK | INFO | `Keep-alive: reconnect successful` |
