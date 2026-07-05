use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;

use crate::types::PeerSyncMessage;

pub const MULTICAST_GROUP: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 1);
pub const MULTICAST_PORT: u16 = 5005;

pub struct PeerSync {
    socket: UdpSocket,
}

impl PeerSync {
    pub fn bind() -> Result<Self> {
        let addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, MULTICAST_PORT);
        let socket = UdpSocket::bind(addr)?;
        socket.join_multicast_v4(&MULTICAST_GROUP, &Ipv4Addr::UNSPECIFIED)?;
        Ok(Self { socket })
    }

    pub fn broadcast(&self, device_id: &str, epoch_num: u64, checksum: u16) -> Result<()> {
        let msg = PeerSyncMessage {
            device_id: device_id.to_string(),
            epoch_num,
            timestamp_us: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_micros() as u64,
            checksum,
        };
        let payload = serde_json::to_vec(&msg)?;
        let addr = SocketAddrV4::new(MULTICAST_GROUP, MULTICAST_PORT);
        self.socket.send_to(&payload, addr)?;
        Ok(())
    }

    pub fn recv(&self, timeout: Duration) -> Result<Option<PeerSyncMessage>> {
        self.socket.set_read_timeout(Some(timeout))?;
        let mut buf = [0u8; 512];
        match self.socket.recv_from(&mut buf) {
            Ok((n, _)) => Ok(Some(serde_json::from_slice(&buf[..n])?)),
            Err(err)
                if err.kind() == std::io::ErrorKind::WouldBlock
                    || err.kind() == std::io::ErrorKind::TimedOut =>
            {
                Ok(None)
            }
            Err(err) => Err(err.into()),
        }
    }
}
