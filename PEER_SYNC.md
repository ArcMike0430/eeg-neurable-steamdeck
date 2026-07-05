# PEER_SYNC

Steam Deck and Jetson are independent MW75 clients. Optional synchronization uses UDP multicast:

- Group: `224.0.0.1:5005`
- Message: `{device_id, epoch_num, timestamp_us, checksum}`
- Behavior: best-effort async sync; standalone operation continues if sync is unavailable.
