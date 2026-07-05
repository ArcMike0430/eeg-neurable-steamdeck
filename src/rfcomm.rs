use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use log::warn;

use crate::protocol::{PACKET_SIZE, STALL_THRESHOLD};

#[derive(Debug, Clone)]
pub struct RingBuffer {
    inner: Arc<(Mutex<VecDeque<Vec<u8>>>, Condvar)>,
    cap: usize,
}

impl RingBuffer {
    pub fn new(cap: usize) -> Self {
        Self {
            inner: Arc::new((Mutex::new(VecDeque::with_capacity(cap)), Condvar::new())),
            cap,
        }
    }

    pub fn push_frame(&self, frame: Vec<u8>) {
        let (lock, cv) = &*self.inner;
        let mut q = lock
            .lock()
            .expect("RingBuffer lock poisoned during push_frame");
        if q.len() >= self.cap {
            q.pop_front();
        }
        q.push_back(frame);
        cv.notify_one();
    }

    pub fn pop_frame_timeout(&self, timeout: Duration) -> Option<Vec<u8>> {
        let (lock, cv) = &*self.inner;
        let mut q = lock
            .lock()
            .expect("RingBuffer lock poisoned during pop_frame_timeout");
        if q.is_empty() {
            let (new_q, _) = cv
                .wait_timeout(q, timeout)
                .expect("RingBuffer condvar poisoned during pop_frame_timeout");
            q = new_q;
        }
        q.pop_front()
    }
}

pub struct RfcommReader {
    running: Arc<AtomicBool>,
    pub ring: RingBuffer,
}

impl RfcommReader {
    pub fn new(capacity: usize) -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            ring: RingBuffer::new(capacity),
        }
    }

    pub fn spawn_from_generator<F>(&self, mut next_frame: F) -> thread::JoinHandle<()>
    where
        F: FnMut() -> Option<Vec<u8>> + Send + 'static,
    {
        let running = Arc::clone(&self.running);
        let ring = self.ring.clone();
        self.running.store(true, Ordering::SeqCst);

        thread::spawn(move || {
            let mut last_rx = Instant::now();
            while running.load(Ordering::SeqCst) {
                match next_frame() {
                    Some(frame) if frame.len() == PACKET_SIZE => {
                        ring.push_frame(frame);
                        last_rx = Instant::now();
                    }
                    Some(_) => {
                        warn!("dropping invalid RFCOMM frame size");
                    }
                    None => thread::sleep(Duration::from_millis(2)),
                }

                if last_rx.elapsed() > STALL_THRESHOLD {
                    warn!("RFCOMM stall detector triggered (>50ms without packet)");
                    last_rx = Instant::now();
                }
            }
        })
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::RingBuffer;

    #[test]
    fn ring_buffer_drops_oldest() {
        let rb = RingBuffer::new(2);
        rb.push_frame(vec![1]);
        rb.push_frame(vec![2]);
        rb.push_frame(vec![3]);

        assert_eq!(
            rb.pop_frame_timeout(Duration::from_millis(1)),
            Some(vec![2])
        );
        assert_eq!(
            rb.pop_frame_timeout(Duration::from_millis(1)),
            Some(vec![3])
        );
    }
}
