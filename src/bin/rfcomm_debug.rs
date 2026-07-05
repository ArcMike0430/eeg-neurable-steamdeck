//! `rfcomm-debug` – RFCOMM protocol debugging tool.
//!
//! Connects to the MW75 over RFCOMM and prints raw packet data, hex dumps,
//! and frame statistics for protocol analysis.
//!
//! Requires the `rfcomm` Cargo feature (Linux / BlueZ).
//!
//! ```bash
//! rfcomm-debug --address AA:BB:CC:DD:EE:FF
//! rfcomm-debug --address AA:BB:CC:DD:EE:FF --hex  # raw hex output
//! ```

#[cfg(not(feature = "rfcomm"))]
fn main() {
    eprintln!("rfcomm-debug requires the `rfcomm` Cargo feature.");
    eprintln!("Rebuild with: cargo build --features rfcomm --bin rfcomm-debug");
    std::process::exit(1);
}

#[cfg(feature = "rfcomm")]
mod inner {
    use anyhow::{Context, Result};
    use bluer::{rfcomm::Socket, rfcomm::SocketAddr, Address};
    use clap::Parser;
    use std::str::FromStr;
    use tokio::io::AsyncReadExt;

    use eeg_neurable_steamdeck::{
        logging,
        parse::{checksum, FrameSync},
        protocol::{PACKET_SIZE, RFCOMM_CHANNEL},
    };

    #[derive(Parser, Debug)]
    #[command(
        name = "rfcomm-debug",
        version,
        about = "RFCOMM protocol debugger for Neurable MW75 (Linux/BlueZ)"
    )]
    pub struct Args {
        /// Bluetooth MAC address of the MW75
        #[arg(short, long, env = "MW75_ADDRESS")]
        pub address: String,

        /// RFCOMM channel
        #[arg(long, default_value_t = RFCOMM_CHANNEL)]
        pub channel: u8,

        /// Print raw hex bytes for each read
        #[arg(long)]
        pub hex: bool,

        /// Maximum packets to receive (0 = unlimited)
        #[arg(short = 'n', long, default_value_t = 0)]
        pub count: u64,

        /// Verbose logging
        #[arg(short, long)]
        pub verbose: bool,
    }

    pub async fn run(args: Args) -> Result<()> {
        if args.verbose {
            logging::init_with_level(log::LevelFilter::Trace);
        } else {
            logging::init();
        }

        let addr = Address::from_str(&args.address)
            .with_context(|| format!("invalid address: {}", args.address))?;

        let socket = Socket::new().context("RFCOMM socket create failed")?;
        let peer = SocketAddr::new(addr, args.channel);

        println!("Connecting to {} ch={}…", args.address, args.channel);
        let mut stream = socket.connect(peer).await.context("RFCOMM connect failed")?;
        println!("Connected. Reading packets (Ctrl-C to stop)…\n");

        let mut read_buf = vec![0u8; PACKET_SIZE * 8];
        let mut sync = FrameSync::new();
        let mut total_bytes = 0u64;
        let mut total_packets = 0u64;
        let mut total_errors = 0u64;
        let mut last_counter: Option<u8> = None;
        let mut lost_packets = 0u64;

        loop {
            let n = stream.read(&mut read_buf).await?;
            if n == 0 {
                println!("\nRemote closed the connection.");
                break;
            }
            total_bytes += n as u64;

            if args.hex {
                print!("[{n} bytes] ");
                for b in &read_buf[..n] {
                    print!("{b:02X} ");
                }
                println!();
            }

            let mut pkts = Vec::new();
            let mut errs = Vec::new();
            sync.feed(&read_buf[..n], &mut pkts, &mut errs);

            for pkt in pkts {
                total_packets += 1;

                if let Some(last) = last_counter {
                    let expected = last.wrapping_add(1);
                    if pkt.counter != expected {
                        lost_packets += 1;
                        println!(
                            "  [WARN] packet gap: expected counter {expected}, got {}",
                            pkt.counter
                        );
                    }
                }
                last_counter = Some(pkt.counter);

                println!(
                    "PKT #{total_packets:>6}  counter={:>3}  status={:#04x}  ch[0]={:.2}µV  ch[11]={:.2}µV",
                    pkt.counter, pkt.status, pkt.channels[0], pkt.channels[11]
                );

                if args.count > 0 && total_packets >= args.count {
                    println!("\n-- Reached limit of {} packets --", args.count);
                    break;
                }
            }

            for err in errs {
                total_errors += 1;
                println!("  [ERR] {err}");
            }

            if args.count > 0 && total_packets >= args.count {
                break;
            }
        }

        println!("\n── Summary ─────────────────────────────────");
        println!("  Total bytes   : {total_bytes}");
        println!("  Total packets : {total_packets}");
        println!("  Parse errors  : {total_errors}");
        println!("  Lost packets  : {lost_packets}");
        if total_packets > 0 {
            let loss_pct = lost_packets as f64 / total_packets as f64 * 100.0;
            println!("  Packet loss   : {loss_pct:.2}%");
        }

        Ok(())
    }
}

#[cfg(feature = "rfcomm")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use clap::Parser;
    let args = inner::Args::parse();
    inner::run(args).await
}
