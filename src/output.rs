//! Output writers: CSV file, WebSocket server, and LSL outlet.

use anyhow::Result;
use chrono::Utc;
use log::{debug, error, info};
use std::path::PathBuf;
use tokio::sync::mpsc;

use crate::types::{EegPacket, Mw75Event};

// ── CSV Writer ───────────────────────────────────────────────────────────────

/// Write EEG packets to a CSV file.
pub struct CsvWriter {
    inner: csv::Writer<std::fs::File>,
}

impl CsvWriter {
    /// Open (or create) a CSV file at `path` and write the header row.
    pub fn new(path: &PathBuf) -> Result<Self> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        let mut wtr = csv::Writer::from_writer(file);

        // Header
        let mut header = vec![
            "Timestamp".to_string(),
            "EventId".to_string(),
            "Counter".to_string(),
            "Ref".to_string(),
            "DRL".to_string(),
        ];
        for i in 1..=12 {
            header.push(format!("Ch{i}RawEEG"));
        }
        header.push("FeatureStatus".to_string());
        wtr.write_record(&header)?;
        wtr.flush()?;

        Ok(Self { inner: wtr })
    }

    /// Write a single EEG packet as a CSV row.
    pub fn write_packet(&mut self, pkt: &EegPacket) -> Result<()> {
        let mut record = vec![
            pkt.timestamp.to_rfc3339(),
            "1".to_string(), // EVT_EEG = 0x01
            pkt.counter.to_string(),
            pkt.ref_uv.to_string(),
            pkt.drl_uv.to_string(),
        ];
        for ch in &pkt.channels {
            record.push(ch.to_string());
        }
        record.push(pkt.status.to_string());
        self.inner.write_record(&record)?;
        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        self.inner.flush()?;
        Ok(())
    }
}

// ── WebSocket Writer ─────────────────────────────────────────────────────────

#[cfg(feature = "websocket")]
pub mod websocket {
    use super::*;
    use futures::{SinkExt, StreamExt};
    use std::net::SocketAddr;
    use tokio::net::TcpListener;
    use tokio_tungstenite::{accept_async, tungstenite::Message};

    /// Serve a WebSocket endpoint that broadcasts EEG events as JSON.
    ///
    /// Each connected client receives a stream of newline-delimited JSON
    /// objects, one per `Mw75Event::Eeg` packet.
    pub async fn serve(bind_addr: SocketAddr, mut rx: mpsc::Receiver<Mw75Event>) -> Result<()> {
        let listener = TcpListener::bind(bind_addr).await?;
        info!("WebSocket server listening on {bind_addr}");

        // Broadcast channel for connected clients
        let (bcast_tx, _) = tokio::sync::broadcast::channel::<String>(1024);
        let bcast_tx_clone = bcast_tx.clone();

        // Pump events into the broadcast channel
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                if let Ok(json) = serde_json::to_string(&event) {
                    let _ = bcast_tx_clone.send(json);
                }
            }
        });

        loop {
            let (tcp, peer) = listener.accept().await?;
            debug!("WebSocket client connected from {peer}");
            let mut client_rx = bcast_tx.subscribe();

            tokio::spawn(async move {
                match accept_async(tcp).await {
                    Ok(ws) => {
                        let (mut ws_tx, _ws_rx) = ws.split();
                        loop {
                            match client_rx.recv().await {
                                Ok(msg) => {
                                    if ws_tx.send(Message::Text(msg.into())).await.is_err() {
                                        break;
                                    }
                                }
                                Err(_) => break,
                            }
                        }
                    }
                    Err(e) => error!("WebSocket handshake error from {peer}: {e}"),
                }
            });
        }
    }
}

// ── LSL Writer ───────────────────────────────────────────────────────────────

#[cfg(feature = "lsl")]
pub mod lsl_output {
    use super::*;
    use lsl::{ChannelFormat, Pushable, StreamInfo, StreamOutlet};

    /// Publish EEG samples to an LSL outlet named `"EEG_MW75"`.
    pub async fn publish(mut rx: mpsc::Receiver<Mw75Event>) -> Result<()> {
        let info = StreamInfo::new(
            "EEG_MW75",
            "EEG",
            12,      // channel count
            500.0,   // nominal sample rate
            ChannelFormat::Float32,
            "MW75-EEG",
        )?;

        let outlet = StreamOutlet::new(&info, 0, 360)?;
        info!("LSL outlet 'EEG_MW75' open");

        while let Some(event) = rx.recv().await {
            if let Mw75Event::Eeg(pkt) = event {
                outlet.push_sample(&pkt.channels.to_vec())?;
            }
        }

        Ok(())
    }
}

// ── Multi-sink dispatcher ────────────────────────────────────────────────────

/// Configuration for which output sinks are enabled.
#[derive(Debug, Default)]
pub struct OutputConfig {
    pub csv_path: Option<PathBuf>,
    #[cfg(feature = "websocket")]
    pub websocket_addr: Option<std::net::SocketAddr>,
    #[cfg(feature = "lsl")]
    pub lsl_enabled: bool,
}

/// Dispatch events from `rx` to all configured output sinks.
pub async fn run_outputs(config: OutputConfig, mut rx: mpsc::Receiver<Mw75Event>) -> Result<()> {
    let mut csv_writer = config
        .csv_path
        .as_ref()
        .map(|p| CsvWriter::new(p))
        .transpose()?;

    let mut packet_count = 0u64;
    let mut last_flush = Utc::now();

    while let Some(event) = rx.recv().await {
        match &event {
            Mw75Event::Eeg(pkt) => {
                packet_count += 1;

                if let Some(wtr) = &mut csv_writer {
                    if let Err(e) = wtr.write_packet(pkt) {
                        error!("CSV write error: {e}");
                    }
                }

                // Flush every 1 000 packets or every second
                let now = Utc::now();
                if packet_count % 1000 == 0
                    || (now - last_flush).num_milliseconds() > 1000
                {
                    if let Some(wtr) = &mut csv_writer {
                        wtr.flush().ok();
                    }
                    last_flush = now;
                    debug!("Packets: {packet_count}");
                }
            }
            Mw75Event::Connected { device_name, address } => {
                info!("Connected: {device_name} @ {address}");
            }
            Mw75Event::Disconnected { address } => {
                info!("Disconnected: {address}");
            }
            Mw75Event::Battery(b) => {
                info!("Battery: {}% (charging: {})", b.level_pct, b.is_charging);
            }
            Mw75Event::StreamStarted => info!("Stream started"),
            Mw75Event::StreamStopped => info!("Stream stopped"),
            Mw75Event::Error(e) => error!("Protocol error: {e}"),
        }
    }

    // Final flush
    if let Some(wtr) = &mut csv_writer {
        wtr.flush().ok();
    }
    info!("Output handler finished. Total packets: {packet_count}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::io::Read;
    use tempfile::NamedTempFile;

    fn make_packet(counter: u8) -> EegPacket {
        EegPacket {
            timestamp: Utc::now(),
            counter,
            ref_uv: 0.0,
            drl_uv: 0.0,
            channels: [1.0; 12],
            status: 0,
            checksum: 0,
        }
    }

    #[test]
    fn csv_writer_creates_header_and_rows() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        let mut wtr = CsvWriter::new(&path).unwrap();

        wtr.write_packet(&make_packet(0)).unwrap();
        wtr.write_packet(&make_packet(1)).unwrap();
        wtr.flush().unwrap();

        let mut s = String::new();
        tmp.reopen().unwrap().read_to_string(&mut s).unwrap();

        let lines: Vec<&str> = s.lines().collect();
        // header + 2 data rows
        assert_eq!(lines.len(), 3, "expected 3 lines, got:\n{s}");
        assert!(lines[0].starts_with("Timestamp"), "missing header");
        assert!(lines[1].contains(",0,"), "counter 0 missing");
        assert!(lines[2].contains(",1,"), "counter 1 missing");
    }
}
