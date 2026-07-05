//! RFCOMM reader — ring-buffer backpressure, stall detection, heartbeat routing.
//!
//! ## Architecture
//!
//! A dedicated Tokio task reads raw bytes from the RFCOMM socket (or
//! simulation source), classifies each packet, and:
//! - Pushes EEG frames into a bounded ring buffer (drops oldest when full).
//! - Forwards heartbeat packets to the keep-alive watchdog channel.
//!
//! The ring buffer absorbs bursts and applies backpressure: when it is full
//! the oldest frames are silently dropped so the reader is never stalled.
//!
//! ## Feature gate
//!
//! Real RFCOMM socket I/O is only compiled with `--features rfcomm`.
//! Without that feature the module exports the same types but
//! `RfcommReader::open()` returns an error in all non-simulation paths.

use crate::keepalive::HeartbeatEvent;
use crate::protocol::{
    FRAME_SIZE, RING_BUFFER_FRAMES,
    STALL_THRESHOLD_MS,
};
use anyhow::{anyhow, Result};
use log::{debug, warn};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

// ---- Ring buffer type -------------------------------------------------------

/// Thread-safe ring buffer of raw RFCOMM EEG frames.
#[derive(Clone)]
pub struct FrameRingBuffer {
    inner: Arc<Mutex<VecDeque<Vec<u8>>>>,
    capacity: usize,
}

impl FrameRingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::with_capacity(capacity))),
            capacity,
        }
    }

    /// Push a frame, dropping the oldest if the buffer is full.
    pub fn push(&self, frame: Vec<u8>) {
        let mut q = self.inner.lock().unwrap();
        if q.len() >= self.capacity {
            q.pop_front(); // drop oldest
        }
        q.push_back(frame);
    }

    /// Pop the oldest frame (returns None if empty).
    pub fn pop(&self) -> Option<Vec<u8>> {
        self.inner.lock().unwrap().pop_front()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ---- Packet classification --------------------------------------------------

enum PacketKind {
    EegFrame(Vec<u8>),
    Heartbeat,
    Unknown,
}

fn classify_packet(data: &[u8]) -> PacketKind {
    if data.len() == FRAME_SIZE && data[0] == 0xA0 && data[1] == 0xA0 {
        PacketKind::EegFrame(data.to_vec())
    } else if data.len() == 1 && data[0] == 0xBB {
        // Heartbeat marker byte (observed on RFCOMM ch1)
        PacketKind::Heartbeat
    } else {
        PacketKind::Unknown
    }
}

// ---- RfcommReader -----------------------------------------------------------

/// Drives RFCOMM I/O and fills the shared ring buffer.
pub struct RfcommReader {
    pub ring: FrameRingBuffer,
    heartbeat_tx: mpsc::Sender<HeartbeatEvent>,
    last_frame_at: Instant,
    pub frames_received: u64,
    pub stalls_detected: u64,
}

impl RfcommReader {
    /// Create a new reader backed by the given ring buffer.
    pub fn new(heartbeat_tx: mpsc::Sender<HeartbeatEvent>) -> Self {
        Self {
            ring: FrameRingBuffer::new(RING_BUFFER_FRAMES),
            heartbeat_tx,
            last_frame_at: Instant::now(),
            frames_received: 0,
            stalls_detected: 0,
        }
    }

    /// Feed a raw byte slice directly (used by simulation and tests).
    pub fn feed(&mut self, data: &[u8]) {
        match classify_packet(data) {
            PacketKind::EegFrame(frame) => {
                let now = Instant::now();
                let elapsed = now.duration_since(self.last_frame_at);
                if elapsed > Duration::from_millis(STALL_THRESHOLD_MS) {
                    warn!(
                        "RFCOMM stall detected: {:.1} ms since last EEG frame (threshold {}ms)",
                        elapsed.as_millis(),
                        STALL_THRESHOLD_MS
                    );
                    self.stalls_detected += 1;
                }
                self.last_frame_at = now;
                self.frames_received += 1;
                debug!("RFCOMM: EEG frame #{}", self.frames_received);
                self.ring.push(frame);
            }
            PacketKind::Heartbeat => {
                let evt = HeartbeatEvent::now();
                let _ = self.heartbeat_tx.try_send(evt);
                debug!("RFCOMM: heartbeat received");
            }
            PacketKind::Unknown => {
                debug!("RFCOMM: unknown packet ({} bytes)", data.len());
            }
        }
    }

    /// Open a real RFCOMM socket and spawn a reader task.
    ///
    /// Returns `(ring_buffer_handle, task_join_handle)`.
    ///
    /// # Errors
    /// Without `--features rfcomm` this always returns an error directing the
    /// caller to use simulation mode.
    #[allow(unused_variables)]
    pub async fn open(
        device_addr: &str,
        eeg_channel: u8,
        heartbeat_tx: mpsc::Sender<HeartbeatEvent>,
    ) -> Result<(FrameRingBuffer, tokio::task::JoinHandle<()>)> {
        #[cfg(feature = "rfcomm")]
        {
            rfcomm_impl::open_rfcomm(device_addr, eeg_channel, heartbeat_tx).await
        }
        #[cfg(not(feature = "rfcomm"))]
        {
            Err(anyhow!(
                "RFCOMM hardware support not compiled in. \
                 Build with `--features rfcomm` for real hardware, or use \
                 `--features simulation` with `--mock` flag for testing."
            ))
        }
    }
}

// ---- Linux RFCOMM socket implementation (feature = "rfcomm") ---------------

#[cfg(feature = "rfcomm")]
mod rfcomm_impl {
    use super::*;
    use crate::keepalive::HeartbeatEvent;
    use anyhow::Context;
    use nix::sys::socket::{
        bind, connect, socket, AddressFamily, SockFlag, SockProtocol, SockType, SockaddrStorage,
    };
    use std::os::unix::io::{FromRawFd, OwnedFd};

    pub async fn open_rfcomm(
        device_addr: &str,
        channel: u8,
        heartbeat_tx: mpsc::Sender<HeartbeatEvent>,
    ) -> Result<(FrameRingBuffer, tokio::task::JoinHandle<()>)> {
        let addr_bytes = parse_bdaddr(device_addr)
            .with_context(|| format!("Invalid Bluetooth address: {device_addr}"))?;

        let ring = FrameRingBuffer::new(RING_BUFFER_FRAMES);
        let ring_clone = ring.clone();

        let task = tokio::task::spawn_blocking(move || {
            rfcomm_reader_thread(addr_bytes, channel, ring_clone, heartbeat_tx);
        });

        Ok((ring, task))
    }

    fn rfcomm_reader_thread(
        addr_bytes: [u8; 6],
        channel: u8,
        ring: FrameRingBuffer,
        heartbeat_tx: mpsc::Sender<HeartbeatEvent>,
    ) {
        use std::io::Read;
        use std::os::unix::net::UnixStream;

        // AF_BLUETOOTH = 31, BTPROTO_RFCOMM = 3
        let af_bluetooth: i32 = 31;
        let btproto_rfcomm: i32 = 3;

        let fd = unsafe { libc::socket(af_bluetooth, libc::SOCK_STREAM, btproto_rfcomm) };
        if fd < 0 {
            log::error!(
                "RFCOMM: socket() failed: {}",
                std::io::Error::last_os_error()
            );
            return;
        }

        // struct sockaddr_rc { sa_family_t rc_family; bdaddr_t rc_bdaddr; uint8_t rc_channel; }
        #[repr(C)]
        struct SockaddrRc {
            rc_family: libc::sa_family_t,
            rc_bdaddr: [u8; 6],
            rc_channel: u8,
        }

        let addr = SockaddrRc {
            rc_family: af_bluetooth as libc::sa_family_t,
            rc_bdaddr: addr_bytes,
            rc_channel: channel,
        };

        let ret = unsafe {
            libc::connect(
                fd,
                &addr as *const SockaddrRc as *const libc::sockaddr,
                std::mem::size_of::<SockaddrRc>() as libc::socklen_t,
            )
        };
        if ret < 0 {
            log::error!(
                "RFCOMM: connect() to ch{channel} failed: {}",
                std::io::Error::last_os_error()
            );
            unsafe { libc::close(fd) };
            return;
        }

        log::info!("RFCOMM: connected to channel {channel}");

        let mut stream = unsafe { std::fs::File::from_raw_fd(fd) };
        let mut buf = [0u8; FRAME_SIZE];
        let stall_threshold = Duration::from_millis(STALL_THRESHOLD_MS);
        let mut last_rx = Instant::now();
        let mut frames_rx = 0u64;

        loop {
            use std::io::Read;
            match stream.read_exact(&mut buf) {
                Ok(()) => {
                    let now = Instant::now();
                    let gap = now.duration_since(last_rx);
                    if gap > stall_threshold {
                        log::warn!(
                            "RFCOMM stall: {:.1}ms gap (threshold {}ms)",
                            gap.as_millis(),
                            STALL_THRESHOLD_MS
                        );
                    }
                    last_rx = now;
                    frames_rx += 1;

                    if buf[0] == 0xA0 && buf[1] == 0xA0 {
                        ring.push(buf.to_vec());
                    } else if buf[0] == 0xBB {
                        let _ = heartbeat_tx.try_send(HeartbeatEvent::now());
                    }
                }
                Err(e) => {
                    log::error!("RFCOMM: read error after {frames_rx} frames: {e}");
                    break;
                }
            }
        }
    }

    fn parse_bdaddr(s: &str) -> Option<[u8; 6]> {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() != 6 {
            return None;
        }
        let mut bytes = [0u8; 6];
        for (i, p) in parts.iter().enumerate() {
            bytes[i] = u8::from_str_radix(p, 16).ok()?;
        }
        // BlueZ expects bdaddr in reverse byte order
        bytes.reverse();
        Some(bytes)
    }
}

// ---- Tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{OFF_CHECKSUM, SOF_BYTE};
    use byteorder::{ByteOrder, LE};

    fn make_eeg_frame(counter: u8) -> Vec<u8> {
        let mut buf = vec![0u8; FRAME_SIZE];
        buf[0] = SOF_BYTE;
        buf[1] = SOF_BYTE;
        buf[3] = counter;
        // Write checksum
        let cs = crate::parse::checksum(&buf[..OFF_CHECKSUM]);
        LE::write_u16(&mut buf[OFF_CHECKSUM..], cs);
        buf
    }

    #[tokio::test]
    async fn test_ring_buffer_drop_oldest() {
        let ring = FrameRingBuffer::new(3);
        ring.push(vec![1]);
        ring.push(vec![2]);
        ring.push(vec![3]);
        ring.push(vec![4]); // should drop vec![1]
        assert_eq!(ring.pop(), Some(vec![2]));
        assert_eq!(ring.pop(), Some(vec![3]));
        assert_eq!(ring.pop(), Some(vec![4]));
        assert_eq!(ring.pop(), None);
    }

    #[test]
    fn test_feed_eeg_frame() {
        let (tx, _rx) = mpsc::channel(16);
        let mut reader = RfcommReader::new(tx);
        let frame = make_eeg_frame(0);
        reader.feed(&frame);
        assert_eq!(reader.frames_received, 1);
        assert!(!reader.ring.is_empty());
    }

    #[test]
    fn test_feed_heartbeat() {
        let (tx, mut rx) = mpsc::channel(16);
        let mut reader = RfcommReader::new(tx);
        reader.feed(&[0xBB]);
        assert_eq!(reader.frames_received, 0);
        assert!(reader.ring.is_empty());
        // heartbeat should be in channel
        assert!(rx.try_recv().is_ok());
    }
}
