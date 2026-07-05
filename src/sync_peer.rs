//! UDP multicast peer sync — broadcasts 1-second EEG epochs to other devices
//! on the local network.
//!
//! # Topology
//! Each device (Steam Deck, Jetson Orin, …) is **independent**: it acquires
//! its own EEG data from its own connected MW75 and broadcasts finished epochs
//! on `224.0.0.1:5005`.  Devices also listen on the same multicast group to
//! receive epochs from peers and can fuse them locally.
//!
//! There is no central relay.  If multicast sync drops, each device continues
//! standalone acquisition without blocking.
//!
//! # Epoch format (binary, little-endian)
//! ```text
//! [0..8]   magic "EEGEPOCH"
//! [8..12]  epoch_seq  u32 LE — monotonically increasing per device
//! [12..18] device_id  [u8; 6] — last 6 bytes of MAC or user-set ID
//! [18..22] sample_count u32 LE
//! [22..]   f32-LE interleaved channels (12 ch × sample_count)
//! ```

use crate::parse::EegFrame;
use crate::protocol::{EEG_CHANNEL_COUNT, SYNC_EPOCH_SECS, SYNC_MULTICAST_ADDR, SYNC_PORT};
use anyhow::{Context, Result};
use byteorder::{ByteOrder, LE};
use log::{debug, info, warn};
use socket2::{Domain, Protocol, Socket, Type};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::time::Duration;

const EPOCH_MAGIC: &[u8; 8] = b"EEGEPOCH";
const HEADER_SIZE: usize = 22;

/// Serialise an epoch to wire format.
fn encode_epoch(seq: u32, device_id: &[u8; 6], frames: &[EegFrame]) -> Vec<u8> {
    let sample_count = frames.len() as u32;
    let payload_floats = sample_count as usize * EEG_CHANNEL_COUNT;
    let mut buf = Vec::with_capacity(HEADER_SIZE + payload_floats * 4);

    buf.extend_from_slice(EPOCH_MAGIC);
    let mut tmp = [0u8; 4];
    LE::write_u32(&mut tmp, seq);
    buf.extend_from_slice(&tmp);
    buf.extend_from_slice(device_id);
    LE::write_u32(&mut tmp, sample_count);
    buf.extend_from_slice(&tmp);

    for frame in frames {
        for &ch in &frame.channels {
            LE::write_f32(&mut tmp, ch);
            buf.extend_from_slice(&tmp);
        }
    }
    buf
}

/// Decode a received epoch from wire format.
#[derive(Debug, Clone)]
pub struct ReceivedEpoch {
    pub seq: u32,
    pub device_id: [u8; 6],
    pub channels: Vec<[f32; EEG_CHANNEL_COUNT]>,
}

fn decode_epoch(data: &[u8]) -> Option<ReceivedEpoch> {
    if data.len() < HEADER_SIZE || &data[..8] != EPOCH_MAGIC {
        return None;
    }
    let seq = LE::read_u32(&data[8..12]);
    let mut device_id = [0u8; 6];
    device_id.copy_from_slice(&data[12..18]);
    let sample_count = LE::read_u32(&data[18..22]) as usize;
    let expected_len = HEADER_SIZE + sample_count * EEG_CHANNEL_COUNT * 4;
    if data.len() < expected_len {
        return None;
    }
    let mut channels = Vec::with_capacity(sample_count);
    let mut offset = HEADER_SIZE;
    for _ in 0..sample_count {
        let mut ch = [0f32; EEG_CHANNEL_COUNT];
        for c in &mut ch {
            *c = LE::read_f32(&data[offset..]);
            offset += 4;
        }
        channels.push(ch);
    }
    Some(ReceivedEpoch {
        seq,
        device_id,
        channels,
    })
}

/// Peer synchronisation manager.
pub struct PeerSync {
    socket: UdpSocket,
    multicast_addr: SocketAddr,
    device_id: [u8; 6],
    epoch_seq: u32,
    /// Accumulated frames for the current epoch.
    epoch_buf: Vec<EegFrame>,
    epoch_samples: usize,
}

impl PeerSync {
    /// Bind and join the multicast group.
    pub fn new(device_id: [u8; 6]) -> Result<Self> {
        let multicast_ip: Ipv4Addr = SYNC_MULTICAST_ADDR
            .parse()
            .context("parse multicast addr")?;
        let bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), SYNC_PORT);

        let sock = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
            .context("create UDP socket")?;
        sock.set_reuse_address(true).ok();
        #[cfg(all(unix, not(any(target_os = "solaris", target_os = "illumos"))))]
        {
            use std::os::unix::io::AsRawFd;
            let optval: libc::c_int = 1;
            unsafe {
                libc::setsockopt(
                    sock.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_REUSEPORT,
                    &optval as *const _ as *const libc::c_void,
                    std::mem::size_of_val(&optval) as libc::socklen_t,
                );
            }
        }
        sock.bind(&bind_addr.into()).context("bind multicast socket")?;
        sock.join_multicast_v4(&multicast_ip, &Ipv4Addr::UNSPECIFIED)
            .context("join multicast group")?;
        sock.set_read_timeout(Some(Duration::from_millis(50))).ok();

        let socket: UdpSocket = sock.into();

        // Epoch size = SYNC_EPOCH_SECS * EEG_SAMPLE_RATE, but we use frame
        // count for flexibility.  Default: 500 samples / epoch.
        let epoch_samples = (crate::protocol::EEG_SAMPLE_RATE as u64 * SYNC_EPOCH_SECS) as usize;

        let multicast_addr = SocketAddr::new(IpAddr::V4(multicast_ip), SYNC_PORT);

        info!(
            "PeerSync: bound to {}, joined {SYNC_MULTICAST_ADDR}:{SYNC_PORT}",
            bind_addr
        );

        Ok(Self {
            socket,
            multicast_addr,
            device_id,
            epoch_seq: 0,
            epoch_buf: Vec::with_capacity(epoch_samples),
            epoch_samples,
        })
    }

    /// Ingest one EEG frame.  When a full epoch accumulates, broadcast it.
    ///
    /// Errors are logged but not propagated — peer sync is best-effort.
    pub fn push_frame(&mut self, frame: EegFrame) {
        self.epoch_buf.push(frame);
        if self.epoch_buf.len() >= self.epoch_samples {
            self.broadcast_epoch();
        }
    }

    fn broadcast_epoch(&mut self) {
        let frames = std::mem::take(&mut self.epoch_buf);
        self.epoch_buf = Vec::with_capacity(self.epoch_samples);

        let payload = encode_epoch(self.epoch_seq, &self.device_id, &frames);
        match self.socket.send_to(&payload, self.multicast_addr) {
            Ok(n) => {
                debug!("PeerSync: broadcast epoch {} ({n} bytes)", self.epoch_seq);
                self.epoch_seq = self.epoch_seq.wrapping_add(1);
            }
            Err(e) => {
                warn!("PeerSync: broadcast failed (graceful fallback): {e}");
            }
        }
    }

    /// Try to receive an epoch from a peer.  Non-blocking (returns None if
    /// nothing is available).
    pub fn try_recv(&self) -> Option<ReceivedEpoch> {
        let mut buf = vec![0u8; 65536];
        match self.socket.recv_from(&mut buf) {
            Ok((n, _src)) => decode_epoch(&buf[..n]),
            Err(_) => None,
        }
    }
}

// ---- Tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::EegFrame;

    fn dummy_frame(counter: u8) -> EegFrame {
        EegFrame {
            counter,
            ref_voltage: 1.0,
            drl_voltage: 2.0,
            channels: [counter as f32; EEG_CHANNEL_COUNT],
            status: 0,
            interpolated: false,
        }
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let device_id = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
        let frames: Vec<EegFrame> = (0..5).map(|i| dummy_frame(i)).collect();
        let encoded = encode_epoch(42, &device_id, &frames);
        let decoded = decode_epoch(&encoded).expect("decode should succeed");
        assert_eq!(decoded.seq, 42);
        assert_eq!(decoded.device_id, device_id);
        assert_eq!(decoded.channels.len(), 5);
        for (i, ch) in decoded.channels.iter().enumerate() {
            assert!((ch[0] - i as f32).abs() < 1e-5);
        }
    }

    #[test]
    fn test_decode_bad_magic() {
        let mut data = vec![0u8; HEADER_SIZE];
        data[..8].copy_from_slice(b"BADBYTES");
        assert!(decode_epoch(&data).is_none());
    }

    #[test]
    fn test_decode_too_short() {
        assert!(decode_epoch(&[]).is_none());
        assert!(decode_epoch(&[0u8; 4]).is_none());
    }
}
