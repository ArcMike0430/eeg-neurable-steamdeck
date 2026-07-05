//! RFCOMM transport for raw EEG streaming on Linux (BlueZ / bluer backend).
//!
//! This module is compiled only when the `rfcomm` feature is enabled.
//! It opens an RFCOMM socket to channel 25 on the MW75 device, reads raw
//! 63-byte EEG frames, and forwards them through a `tokio::mpsc` channel.

#![cfg(feature = "rfcomm")]

use anyhow::{Context, Result};
use bluer::{
    rfcomm::{Profile, Socket, SocketAddr},
    Address,
};
use log::{debug, error, info, warn};
use std::str::FromStr;
use tokio::{
    io::AsyncReadExt,
    sync::mpsc,
};

use crate::{
    parse::FrameSync,
    protocol::{PACKET_SIZE, RFCOMM_CHANNEL},
    types::{Mw75Event, ProtocolError},
};

// ── Public API ───────────────────────────────────────────────────────────────

/// Open an RFCOMM connection to the MW75 and stream raw EEG events.
///
/// # Arguments
/// * `address` – Bluetooth MAC address of the MW75 (e.g. `"AA:BB:CC:DD:EE:FF"`)
/// * `channel` – RFCOMM channel to connect on (default: 25)
/// * `tx`      – Sender half of the event channel
pub async fn stream_rfcomm(
    address: &str,
    channel: u8,
    tx: mpsc::Sender<Mw75Event>,
) -> Result<()> {
    let addr = Address::from_str(address)
        .with_context(|| format!("invalid Bluetooth address: {address}"))?;

    let socket = Socket::new().context("failed to create RFCOMM socket")?;
    let peer = SocketAddr::new(addr, channel);

    info!("Connecting RFCOMM to {address} ch={channel}…");
    let mut stream = socket
        .connect(peer)
        .await
        .with_context(|| format!("RFCOMM connect to {address}:{channel} failed"))?;
    info!("RFCOMM connected");

    let _ = tx.send(Mw75Event::StreamStarted).await;

    let mut read_buf = vec![0u8; PACKET_SIZE * 8];
    let mut sync = FrameSync::new();

    loop {
        let n = match stream.read(&mut read_buf).await {
            Ok(0) => {
                info!("RFCOMM stream closed by peer");
                break;
            }
            Ok(n) => n,
            Err(e) => {
                error!("RFCOMM read error: {e}");
                let _ = tx
                    .send(Mw75Event::Error(ProtocolError::PacketTooShort {
                        expected: PACKET_SIZE,
                        got: 0,
                    }))
                    .await;
                break;
            }
        };

        debug!("RFCOMM read {n} bytes");

        let mut packets = Vec::new();
        let mut errors = Vec::new();
        sync.feed(&read_buf[..n], &mut packets, &mut errors);

        for pkt in packets {
            if tx.send(Mw75Event::Eeg(pkt)).await.is_err() {
                return Ok(());
            }
        }

        for err in errors {
            warn!("Parse error: {err}");
            if tx.send(Mw75Event::Error(err)).await.is_err() {
                return Ok(());
            }
        }
    }

    let _ = tx.send(Mw75Event::StreamStopped).await;
    Ok(())
}
