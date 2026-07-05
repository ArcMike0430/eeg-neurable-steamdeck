//! `eeg-stream` — main EEG acquisition and streaming binary.
//!
//! # Usage
//!
//! Mock mode (no hardware, simulation only):
//! ```sh
//! cargo run --bin eeg-stream --features simulation -- --mock
//! cargo run --bin eeg-stream --features simulation -- --mock --drop-rate 0.05
//! cargo run --bin eeg-stream --features simulation -- --mock --corruption-rate 0.1 --stress-reconnect
//! ```
//!
//! Hardware mode (USB-BT500 + MW75 required):
//! ```sh
//! cargo run --bin eeg-stream --features rfcomm
//! cargo run --bin eeg-stream --features rfcomm -- --config config/steam-deck.toml
//! ```

use anyhow::{Context, Result};
use eeg_neurable_steamdeck::prelude::*;
use eeg_neurable_steamdeck::{adapter, config as cfg_mod, init_logger};
use log::{error, info, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

// ---- CLI --------------------------------------------------------------------

#[derive(Debug, Default)]
struct Args {
    config_path: Option<String>,
    mock: bool,
    drop_rate: f64,
    jitter_ms: u64,
    corruption_rate: f64,
    timeout_after: Option<u64>,
    stress_reconnect: bool,
}

fn parse_args() -> Args {
    let mut args = Args::default();
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < raw.len() {
        match raw[i].as_str() {
            "--config" => {
                i += 1;
                if i < raw.len() {
                    args.config_path = Some(raw[i].clone());
                }
            }
            "--mock" => args.mock = true,
            "--drop-rate" => {
                i += 1;
                if i < raw.len() {
                    args.drop_rate = raw[i].parse().unwrap_or(0.0);
                }
            }
            "--jitter-ms" => {
                i += 1;
                if i < raw.len() {
                    args.jitter_ms = raw[i].parse().unwrap_or(0);
                }
            }
            "--corruption-rate" => {
                i += 1;
                if i < raw.len() {
                    args.corruption_rate = raw[i].parse().unwrap_or(0.0);
                }
            }
            "--timeout-after" => {
                i += 1;
                if i < raw.len() {
                    args.timeout_after = raw[i].parse().ok();
                }
            }
            "--stress-reconnect" => args.stress_reconnect = true,
            _ => {}
        }
        i += 1;
    }
    args
}

// ---- Main -------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    let args = parse_args();

    let mut cfg = if let Some(path) = &args.config_path {
        cfg_mod::load(path).with_context(|| format!("load config {path}"))?
    } else {
        Config::default()
    };

    // CLI overrides
    if args.mock {
        cfg.simulation.enabled = true;
    }
    if args.drop_rate > 0.0 {
        cfg.simulation.drop_rate = args.drop_rate;
    }
    if args.jitter_ms > 0 {
        cfg.simulation.jitter_ms = args.jitter_ms;
    }
    if args.corruption_rate > 0.0 {
        cfg.simulation.corruption_rate = args.corruption_rate;
    }
    if let Some(t) = args.timeout_after {
        cfg.simulation.timeout_after = Some(t);
    }
    if args.stress_reconnect {
        cfg.simulation.stress_reconnect = true;
    }

    init_logger(&cfg.logging.level);

    info!(
        "eeg-stream starting — platform='{}' node='{}'",
        cfg.device.platform, cfg.device.name
    );

    // Ctrl-C handler
    let running = Arc::new(AtomicBool::new(true));
    {
        let r = running.clone();
        tokio::spawn(async move {
            tokio::signal::ctrl_c()
                .await
                .expect("Failed to listen for ctrl-c");
            info!("Shutdown signal received");
            r.store(false, Ordering::Relaxed);
        });
    }

    if cfg.simulation.enabled {
        run_simulation(&cfg, running).await
    } else {
        run_hardware(&cfg, running).await
    }
}

// ---- Simulation loop --------------------------------------------------------

async fn run_simulation(cfg: &Config, running: Arc<AtomicBool>) -> Result<()> {
    info!(
        "MOCK MODE — drop={:.0}% jitter={}ms corrupt={:.0}% timeout={:?} stress={}",
        cfg.simulation.drop_rate * 100.0,
        cfg.simulation.jitter_ms,
        cfg.simulation.corruption_rate * 100.0,
        cfg.simulation.timeout_after,
        cfg.simulation.stress_reconnect,
    );

    let sim_cfg = SimConfig {
        drop_rate: cfg.simulation.drop_rate,
        jitter_ms: cfg.simulation.jitter_ms,
        corruption_rate: cfg.simulation.corruption_rate,
        timeout_after: cfg.simulation.timeout_after,
        stress_reconnect: cfg.simulation.stress_reconnect,
        sample_rate: eeg_neurable_steamdeck::protocol::EEG_SAMPLE_RATE,
    };

    let mut sim = EegSimulator::new(sim_cfg.clone());
    let mut parser = FrameParser::new();

    let mut peer_sync = try_init_peer_sync(cfg);

    let mut frames_total: u64 = 0;
    let report_every = 500u64;

    while running.load(Ordering::Relaxed) {
        eeg_neurable_steamdeck::simulate::sim_frame_delay(&sim_cfg).await;

        match sim.next_raw() {
            None => {
                info!("Simulation timeout limit reached — exiting");
                break;
            }
            Some(raw) => {
                match parser.parse(&raw) {
                    Ok(frames) => {
                        for frame in frames {
                            frames_total += 1;
                            if frames_total % report_every == 0 {
                                info!(
                                    "Frame #{frames_total} counter={} ref={:.3}µV ch0={:.3}µV{}",
                                    frame.counter,
                                    frame.ref_voltage,
                                    frame.channels[0],
                                    if frame.interpolated { " [interp]" } else { "" }
                                );
                                log_stats(&parser.stats);
                            }
                            if let Some(ps) = &mut peer_sync {
                                ps.push_frame(frame);
                            }
                        }
                    }
                    Err(e) => {
                        log::debug!("Parse error (expected in mock): {e}");
                    }
                }
            }
        }
    }

    info!("eeg-stream done — frames={frames_total} stats={:?}", parser.stats);
    Ok(())
}

// ---- Hardware loop ----------------------------------------------------------

async fn run_hardware(cfg: &Config, running: Arc<AtomicBool>) -> Result<()> {
    // Adapter selection
    let adapters = adapter::enumerate_adapters();
    let _ = adapter::prefer_usb_adapter(&adapters);

    // Heartbeat channel
    let (hb_tx, hb_rx) = mpsc::channel::<HeartbeatEvent>(64);

    // BLE connect + GAIA activation
    let peripheral = eeg_neurable_steamdeck::mw75_client::find_and_connect(cfg.ble.adapter_index)
        .await
        .context("BLE connect to MW75")?;

    eeg_neurable_steamdeck::mw75_client::activate_gaia(&peripheral)
        .await
        .context("GAIA activation")?;

    eeg_neurable_steamdeck::mw75_client::query_battery(&peripheral).await;

    // RFCOMM EEG ring buffer
    let ring = open_rfcomm_or_stub(cfg, hb_tx).await;

    // Keep-alive watchdog
    let adapter_idx = cfg.ble.adapter_index;
    {
        let mut watchdog = KeepAliveWatchdog::new(hb_rx);
        tokio::spawn(async move {
            watchdog
                .run(|| async move {
                    warn!("Watchdog: triggering reconnect");
                    match eeg_neurable_steamdeck::mw75_client::reconnect_with_backoff(adapter_idx)
                        .await
                    {
                        Ok(_) => info!("Watchdog: reconnect OK"),
                        Err(e) => error!("Watchdog: reconnect failed: {e:#}"),
                    }
                })
                .await;
        });
    }

    let mut peer_sync = try_init_peer_sync(cfg);
    let mut parser = FrameParser::new();
    let mut frames_total: u64 = 0;
    let report_every = 500u64;

    while running.load(Ordering::Relaxed) {
        if let Some(raw) = ring.pop() {
            match parser.parse(&raw) {
                Ok(frames) => {
                    for frame in frames {
                        frames_total += 1;
                        if frames_total % report_every == 0 {
                            info!(
                                "Frame #{frames_total} counter={} ref={:.3}µV ch0={:.3}µV{}",
                                frame.counter,
                                frame.ref_voltage,
                                frame.channels[0],
                                if frame.interpolated { " [interp]" } else { "" }
                            );
                            log_stats(&parser.stats);
                        }
                        if let Some(ps) = &mut peer_sync {
                            ps.push_frame(frame);
                        }
                    }
                }
                Err(e) => warn!("Parse error: {e}"),
            }
        } else {
            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        }
    }

    info!("eeg-stream done — frames={frames_total}");
    Ok(())
}

// ---- Helpers ----------------------------------------------------------------

fn try_init_peer_sync(cfg: &Config) -> Option<PeerSync> {
    if !cfg.sync.enabled {
        return None;
    }
    let device_id = name_to_device_id(&cfg.device.name);
    match PeerSync::new(device_id) {
        Ok(ps) => {
            info!(
                "PeerSync active — multicast {}:{}",
                cfg.sync.multicast_addr, cfg.sync.port
            );
            Some(ps)
        }
        Err(e) => {
            warn!("PeerSync unavailable ({e}) — continuing without sync");
            None
        }
    }
}

fn name_to_device_id(name: &str) -> [u8; 6] {
    let mut id = [0u8; 6];
    for (i, b) in name.bytes().take(6).enumerate() {
        id[i] = b;
    }
    id
}

fn log_stats(stats: &ParseStats) {
    info!(
        "  ok={} bad_sof={} bad_cksum={} gap_fill={} gap_lost={}",
        stats.frames_ok,
        stats.frames_bad_sof,
        stats.frames_bad_checksum,
        stats.gaps_filled,
        stats.gaps_lost,
    );
}

async fn open_rfcomm_or_stub(
    cfg: &Config,
    hb_tx: mpsc::Sender<HeartbeatEvent>,
) -> FrameRingBuffer {
    #[cfg(feature = "rfcomm")]
    {
        use btleplug::api::Peripheral as _;
        // peripheral address obtained before calling this function
        // For now use placeholder; in practice pass the address through
        let addr = "00:00:00:00:00:00";
        match RfcommReader::open(addr, cfg.rfcomm.eeg_channel, hb_tx).await {
            Ok((ring, _task)) => return ring,
            Err(e) => {
                warn!("RFCOMM open failed ({e}) — empty ring buffer");
            }
        }
    }
    #[cfg(not(feature = "rfcomm"))]
    {
        warn!(
            "RFCOMM feature not enabled. Build with `--features rfcomm` for hardware support."
        );
    }
    let _ = hb_tx; // suppress unused warning
    FrameRingBuffer::new(cfg.rfcomm.ring_buffer_size)
}
