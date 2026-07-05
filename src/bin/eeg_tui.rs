//! `eeg-tui` – real-time terminal waveform viewer for Neurable MW75.
//!
//! Requires the `tui` Cargo feature.
//!
//! Controls:
//!   `p`  – pause / resume display
//!   `q`  – quit
//!   `+`  – increase vertical scale
//!   `-`  – decrease vertical scale

use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::Span,
    widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, Paragraph},
    Frame, Terminal,
};
use std::{
    collections::VecDeque,
    io,
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

use eeg_neurable_steamdeck::{
    logging,
    simulate::spawn_simulator,
    types::{BatteryInfo, Mw75Event},
};

// ── Constants ────────────────────────────────────────────────────────────────

const DISPLAY_CHANNELS: usize = 4;
const HISTORY_SAMPLES: usize = 500; // 1 second at 500 Hz
const REFRESH_RATE_MS: u64 = 33; // ~30 fps

// ── CLI args ─────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(
    name = "eeg-tui",
    version,
    about = "Real-time terminal EEG waveform viewer for Neurable MW75"
)]
struct Args {
    /// Bluetooth MAC address of the MW75
    #[arg(short, long, env = "MW75_ADDRESS")]
    address: Option<String>,

    /// Enable simulation mode (no hardware required)
    #[arg(long)]
    simulate: bool,

    /// Initial vertical scale in µV
    #[arg(long, default_value_t = 50.0)]
    scale: f64,

    /// Verbose logging
    #[arg(short, long)]
    verbose: bool,
}

// ── App state ────────────────────────────────────────────────────────────────

struct App {
    /// Circular buffers for each displayed channel (x, y) data
    channels: [VecDeque<(f64, f64)>; DISPLAY_CHANNELS],
    paused: bool,
    scale_uv: f64,
    packet_count: u64,
    error_count: u64,
    battery: Option<BatteryInfo>,
    sample_x: f64,
    last_counter: Option<u8>,
    lost_packets: u64,
}

impl App {
    fn new(scale_uv: f64) -> Self {
        Self {
            channels: std::array::from_fn(|_| VecDeque::with_capacity(HISTORY_SAMPLES + 16)),
            paused: false,
            scale_uv,
            packet_count: 0,
            error_count: 0,
            battery: None,
            sample_x: 0.0,
            last_counter: None,
            lost_packets: 0,
        }
    }

    fn push_event(&mut self, event: Mw75Event) {
        match event {
            Mw75Event::Eeg(pkt) => {
                // Track lost packets
                if let Some(last) = self.last_counter {
                    let expected = last.wrapping_add(1);
                    if pkt.counter != expected {
                        self.lost_packets = self.lost_packets.saturating_add(1);
                    }
                }
                self.last_counter = Some(pkt.counter);
                self.packet_count += 1;

                if !self.paused {
                    for (i, buf) in self.channels.iter_mut().enumerate() {
                        buf.push_back((self.sample_x, pkt.channels[i] as f64));
                        if buf.len() > HISTORY_SAMPLES {
                            buf.pop_front();
                        }
                    }
                    self.sample_x += 1.0 / 500.0; // seconds
                }
            }
            Mw75Event::Battery(b) => {
                self.battery = Some(b);
            }
            Mw75Event::Error(_) => {
                self.error_count += 1;
            }
            _ => {}
        }
    }
}

// ── Rendering ────────────────────────────────────────────────────────────────

fn draw(frame: &mut Frame, app: &App) {
    let area = frame.size();

    // Split into status bar + waveform area
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    draw_status(frame, app, chunks[0]);
    draw_waveforms(frame, app, chunks[1]);
}

fn draw_status(frame: &mut Frame, app: &App, area: Rect) {
    let battery_str = app
        .battery
        .as_ref()
        .map(|b| format!("🔋 {}%{}", b.level_pct, if b.is_charging { " ⚡" } else { "" }))
        .unwrap_or_else(|| "🔋 --".to_string());

    let status = format!(
        " Packets: {}  Errors: {}  Lost: {}  Scale: ±{:.0}µV  {}  {}",
        app.packet_count,
        app.error_count,
        app.lost_packets,
        app.scale_uv,
        battery_str,
        if app.paused { "⏸ PAUSED" } else { "▶ LIVE" },
    );

    let paragraph = Paragraph::new(status)
        .block(Block::default().borders(Borders::ALL).title("MW75 EEG Monitor  [p]ause  [+/-]scale  [q]uit"));
    frame.render_widget(paragraph, area);
}

const CHANNEL_COLORS: [Color; DISPLAY_CHANNELS] = [
    Color::Green,
    Color::Cyan,
    Color::Yellow,
    Color::Magenta,
];

fn draw_waveforms(frame: &mut Frame, app: &App, area: Rect) {
    let constraints: Vec<Constraint> = (0..DISPLAY_CHANNELS)
        .map(|_| Constraint::Percentage(100 / DISPLAY_CHANNELS as u16))
        .collect();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    for (i, (buf, &chunk)) in app.channels.iter().zip(chunks.iter()).enumerate() {
        let data: Vec<(f64, f64)> = buf.iter().copied().collect();
        let x_min = data.first().map(|p| p.0).unwrap_or(0.0);
        let x_max = data.last().map(|p| p.0).unwrap_or(1.0);

        let dataset = Dataset::default()
            .name(format!("Ch{}", i + 1))
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(CHANNEL_COLORS[i]))
            .data(&data);

        let chart = Chart::new(vec![dataset])
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(Span::styled(
                        format!(" Channel {} ", i + 1),
                        Style::default()
                            .fg(CHANNEL_COLORS[i])
                            .add_modifier(Modifier::BOLD),
                    )),
            )
            .x_axis(
                Axis::default()
                    .bounds([x_min, x_max])
                    .labels(vec![
                        Span::raw(format!("{x_min:.1}s")),
                        Span::raw(format!("{x_max:.1}s")),
                    ]),
            )
            .y_axis(
                Axis::default()
                    .bounds([-app.scale_uv, app.scale_uv])
                    .labels(vec![
                        Span::raw(format!("-{:.0}µV", app.scale_uv)),
                        Span::raw("0"),
                        Span::raw(format!("+{:.0}µV", app.scale_uv)),
                    ]),
            );

        frame.render_widget(chart, chunk);
    }
}

// ── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    if args.verbose {
        logging::init_with_level(log::LevelFilter::Debug);
    } else {
        logging::init();
    }

    let simulate = args.simulate || args.address.is_none();
    let mut rx: mpsc::Receiver<Mw75Event> = if simulate {
        spawn_simulator(500)
    } else {
        #[cfg(feature = "rfcomm")]
        {
            use eeg_neurable_steamdeck::{mw75_client, rfcomm};
            let (tx, rx) = mpsc::channel(1024);
            let addr = args.address.clone().unwrap();
            tokio::spawn(async move {
                let p = mw75_client::discover_mw75(Duration::from_secs(15)).await?;
                mw75_client::activate_eeg(&p).await?;
                rfcomm::stream_rfcomm(&addr, 25, tx).await
            });
            rx
        }
        #[cfg(not(feature = "rfcomm"))]
        {
            spawn_simulator(500)
        }
    };

    // Set up terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let mut app = App::new(args.scale);
    let tick = Duration::from_millis(REFRESH_RATE_MS);
    let mut last_draw = Instant::now();

    loop {
        // Drain all pending events
        while let Ok(ev) = rx.try_recv() {
            app.push_event(ev);
        }

        // Redraw at refresh rate
        if last_draw.elapsed() >= tick {
            terminal.draw(|f| draw(f, &app))?;
            last_draw = Instant::now();
        }

        // Handle keyboard input (non-blocking)
        if event::poll(Duration::from_millis(5))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Char('Q') => break,
                    KeyCode::Char('p') | KeyCode::Char('P') => app.paused = !app.paused,
                    KeyCode::Char('+') => app.scale_uv = (app.scale_uv * 1.25).min(500.0),
                    KeyCode::Char('-') => app.scale_uv = (app.scale_uv / 1.25).max(5.0),
                    _ => {}
                }
            }
        }

        tokio::task::yield_now().await;
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}
