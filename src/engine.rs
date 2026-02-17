use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;
use rand_distr::{Exp, LogNormal, Poisson, Uniform};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::net::UdpSocket;
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::config::{AppConfig, FileConfig, OutputMode};
use crate::multicast::MulticastSender;
use crate::order::{Order, OrderType, Side};
use crate::regime::{self, Regime, RegimeState};
use crate::scenario::{Scenario, ScenarioConfig};

/// GBM dt: tick interval expressed in years.
fn dt_years(tick_interval: f64) -> f64 {
    1.0 / (252.0 * 6.5 * 3600.0) * tick_interval / 0.1 * 0.1
}

/// Per-interval statistics for display/logging.
struct TickStats {
    limits_generated: u64,
    markets_generated: u64,
    cancels_expired: u64,
    cancels_regime: u64,
    messages_sent: u64,
}

impl TickStats {
    fn new() -> Self {
        Self {
            limits_generated: 0,
            markets_generated: 0,
            cancels_expired: 0,
            cancels_regime: 0,
            messages_sent: 0,
        }
    }

    fn reset(&mut self) {
        self.limits_generated = 0;
        self.markets_generated = 0;
        self.cancels_expired = 0;
        self.cancels_regime = 0;
        self.messages_sent = 0;
    }

    fn total_orders(&self) -> u64 {
        self.limits_generated + self.markets_generated
    }

    fn total_cancels(&self) -> u64 {
        self.cancels_expired + self.cancels_regime
    }
}

const BOX_W: usize = 50;

fn box_line(content: &str) -> String {
    let pad = if content.len() < BOX_W {
        BOX_W - content.len()
    } else {
        0
    };
    format!("│ {}{} │", content, " ".repeat(pad))
}

fn box_top() -> String {
    format!("┌─{}─┐", "─".repeat(BOX_W))
}

fn box_mid() -> String {
    format!("├─{}─┤", "─".repeat(BOX_W))
}

fn box_bottom() -> String {
    format!("└─{}─┘", "─".repeat(BOX_W))
}

enum ControlCommand {
    Pause,
    Resume,
    Throughput(f64),
    DisplayInterval(f64),
    Regime(Regime),
    Reload,
    Stats,
}

struct RuntimeTunables {
    throughput_scale: f64,
    display_interval: f64,
    shock_prob: f64,
    paused: bool,
}

fn parse_regime(s: &str) -> Option<Regime> {
    match s {
        "calm" => Some(Regime::Calm),
        "volatile" => Some(Regime::Volatile),
        "crash" => Some(Regime::Crash),
        "rally" => Some(Regime::Rally),
        "recovery" => Some(Regime::Recovery),
        _ => None,
    }
}

fn parse_control_command(input: &str) -> Option<ControlCommand> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut parts = trimmed.split_whitespace();
    let cmd = parts.next()?.to_ascii_lowercase();
    match cmd.as_str() {
        "pause" => Some(ControlCommand::Pause),
        "resume" => Some(ControlCommand::Resume),
        "reload" => Some(ControlCommand::Reload),
        "stats" => Some(ControlCommand::Stats),
        "rate" | "throughput" => {
            let v = parts.next()?.parse::<f64>().ok()?;
            Some(ControlCommand::Throughput(v))
        }
        "display" => {
            let v = parts.next()?.parse::<f64>().ok()?;
            Some(ControlCommand::DisplayInterval(v))
        }
        "regime" => {
            let r = parse_regime(&parts.next()?.to_ascii_lowercase())?;
            Some(ControlCommand::Regime(r))
        }
        _ => None,
    }
}

fn spawn_control_listener(bind: &str) -> std::io::Result<Receiver<ControlCommand>> {
    let socket = UdpSocket::bind(bind)?;
    socket.set_read_timeout(Some(Duration::from_millis(500)))?;

    let (tx, rx) = mpsc::channel::<ControlCommand>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 1024];
        loop {
            match socket.recv_from(&mut buf) {
                Ok((n, peer)) => {
                    let cmd_text = String::from_utf8_lossy(&buf[..n]).trim().to_string();
                    if let Some(cmd) = parse_control_command(&cmd_text) {
                        let _ = tx.send(cmd);
                        let _ = socket.send_to(b"ok\n", peer);
                    } else {
                        let _ = socket.send_to(
                            b"error: commands are pause|resume|rate <x>|display <sec>|regime <name>|reload|stats\n",
                            peer,
                        );
                    }
                }
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(_) => break,
            }
        }
    });

    Ok(rx)
}

/// Output sink that respects the configured output mode.
struct Output {
    mode: OutputMode,
    file: Option<std::fs::File>,
}

impl Output {
    fn new(cfg: &AppConfig) -> Result<Self, Box<dyn std::error::Error>> {
        let file = match cfg.output_mode {
            OutputMode::File | OutputMode::Both => {
                let f = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&cfg.log_file)
                    .map_err(|e| format!("failed to open log file '{}': {}", cfg.log_file, e))?;
                Some(f)
            }
            _ => None,
        };
        Ok(Self {
            mode: cfg.output_mode,
            file,
        })
    }

    fn to_console(&self) -> bool {
        matches!(self.mode, OutputMode::Console | OutputMode::Both)
    }

    fn to_file(&self) -> bool {
        self.file.is_some()
    }

    fn print(&mut self, msg: &str) {
        if self.to_console() {
            println!("{}", msg);
        }
        if let Some(ref mut f) = self.file {
            let _ = writeln!(f, "{}", msg);
        }
    }

    fn event(&mut self, msg: &str) {
        self.print(msg);
    }

    /// Print the periodic summary block.
    fn summary(
        &mut self,
        elapsed: f64,
        mid: f64,
        regime: Regime,
        active_orders: usize,
        stats: &TickStats,
        interval_secs: f64,
    ) {
        let orders_per_sec = stats.total_orders() as f64 / interval_secs;
        let cancels_per_sec = stats.total_cancels() as f64 / interval_secs;
        let msgs_per_sec = stats.messages_sent as f64 / interval_secs;

        if self.to_console() {
            println!("{}", box_top());
            println!("{}", box_line(&format!(
                "t={:.1}s  mid={:.4}  regime={}",
                elapsed, mid, regime
            )));
            println!("{}", box_line(&format!(
                "orders: {} ({:.0}/s)  limits: {}  mkt: {}",
                stats.total_orders(), orders_per_sec,
                stats.limits_generated, stats.markets_generated
            )));
            println!("{}", box_line(&format!(
                "cancels: {} ({:.0}/s)  expired: {}  regime: {}",
                stats.total_cancels(), cancels_per_sec,
                stats.cancels_expired, stats.cancels_regime
            )));
            println!("{}", box_line(&format!(
                "active: {}  msgs/s: {:.0}",
                active_orders, msgs_per_sec
            )));
            println!("{}", box_bottom());
        }

        if self.to_file() {
            if let Some(ref mut f) = self.file {
                let _ = writeln!(
                    f,
                    "SUMMARY|t={:.1}|mid={:.4}|regime={}|active={}|limits={}|markets={}|cancels_exp={}|cancels_reg={}|msgs={}",
                    elapsed, mid, regime, active_orders,
                    stats.limits_generated, stats.markets_generated,
                    stats.cancels_expired, stats.cancels_regime,
                    stats.messages_sent
                );
            }
        }
    }
}

pub fn run(cfg: &AppConfig) -> Result<(), Box<dyn std::error::Error>> {
    let mut rng = StdRng::seed_from_u64(cfg.seed);

    let scenario_cfg = ScenarioConfig::from_scenario(cfg.scenario);
    let mut state = RegimeState::new(scenario_cfg.starting_regime, &mut rng);
    let mut forced_event_fired = false;

    let sender = MulticastSender::new(cfg.multicast_group, cfg.multicast_port, cfg.wire_format)?;
    let mut out = Output::new(cfg)?;

    let mut runtime = RuntimeTunables {
        throughput_scale: cfg.throughput_scale,
        display_interval: cfg.display_interval,
        shock_prob: cfg.shock_prob,
        paused: false,
    };

    let control_rx = if cfg.control_enabled {
        match spawn_control_listener(&cfg.control_bind) {
            Ok(rx) => {
                out.event(&format!("  ▶ CONTROL API listening on udp://{}", cfg.control_bind));
                Some(rx)
            }
            Err(e) => {
                out.event(&format!("  ⚠ control API disabled: {}", e));
                None
            }
        }
    } else {
        None
    };

    let running = Arc::new(AtomicBool::new(true));
    {
        let running = Arc::clone(&running);
        ctrlc::set_handler(move || {
            running.store(false, Ordering::SeqCst);
        })?;
    }

    // --- Startup banner ---
    out.print(&box_top());
    out.print(&box_line("Order Generation Engine"));
    out.print(&box_mid());
    out.print(&box_line(&format!("scenario:    {}", cfg.scenario)));
    out.print(&box_line(&format!("regime:      {}", state.current)));
    out.print(&box_line(&format!("mid price:   {}", cfg.initial_price)));
    out.print(&box_line(&format!("tick:        {}s", cfg.tick_interval)));
    out.print(&box_line(&format!("seed:        {}", cfg.seed)));
    out.print(&box_line(&format!("throughput:  {}x", runtime.throughput_scale)));
    out.print(&box_line(&format!("output:      {}", cfg.output_mode)));
    out.print(&box_line(&format!("wire fmt:    {}", cfg.wire_format)));
    if out.to_file() {
        out.print(&box_line(&format!("log file:    {}", cfg.log_file)));
    }
    out.print(&box_line(&format!(
        "multicast:   {}:{}",
        cfg.multicast_group, cfg.multicast_port
    )));
    if cfg.control_enabled {
        out.print(&box_line(&format!("control:     udp://{}", cfg.control_bind)));
    }
    out.print(&box_bottom());

    let dt = dt_years(cfg.tick_interval);
    let dt_seconds = cfg.tick_interval;
    let size_dist = LogNormal::new(cfg.size_mean_log, cfg.size_std_log)?;
    let ttl_dist = Uniform::new(cfg.ttl_min, cfg.ttl_max);

    let mut mid = cfg.initial_price;
    let mut next_id: u64 = 0;
    let mut active_orders: HashMap<u64, Order> = HashMap::new();
    let mut current_time: f64 = 0.0;
    let mut last_printed_regime = state.current;

    let mut stats = TickStats::new();
    let mut time_since_display: f64 = 0.0;

    while running.load(Ordering::Relaxed) {
        if let Some(rx) = &control_rx {
            while let Ok(cmd) = rx.try_recv() {
                match cmd {
                    ControlCommand::Pause => {
                        runtime.paused = true;
                        out.event("  ▶ CONTROL pause");
                    }
                    ControlCommand::Resume => {
                        runtime.paused = false;
                        out.event("  ▶ CONTROL resume");
                    }
                    ControlCommand::Throughput(v) if v >= 0.0 => {
                        runtime.throughput_scale = v;
                        out.event(&format!("  ▶ CONTROL throughput={}x", v));
                    }
                    ControlCommand::DisplayInterval(v) if v > 0.0 => {
                        runtime.display_interval = v;
                        out.event(&format!("  ▶ CONTROL display_interval={}s", v));
                    }
                    ControlCommand::Regime(next) => {
                        state.transition_to(next, &mut rng);
                        out.event(&format!("  ▶ CONTROL regime -> {}", state.current));
                    }
                    ControlCommand::Reload => {
                        if let Some(path) = &cfg.config_path {
                            match std::fs::read_to_string(path) {
                                Ok(contents) => match toml::from_str::<FileConfig>(&contents) {
                                    Ok(file_cfg) => {
                                        runtime.throughput_scale = file_cfg.simulation.throughput_scale;
                                        runtime.display_interval = file_cfg.output.display_interval;
                                        runtime.shock_prob = file_cfg.shocks.probability;
                                        out.event(&format!(
                                            "  ▶ CONTROL reload OK throughput={}x display={}s shock_prob={}",
                                            runtime.throughput_scale,
                                            runtime.display_interval,
                                            runtime.shock_prob
                                        ));
                                    }
                                    Err(e) => out.event(&format!("  ⚠ reload parse failed: {}", e)),
                                },
                                Err(e) => out.event(&format!("  ⚠ reload read failed: {}", e)),
                            }
                        } else {
                            out.event("  ⚠ reload unavailable (run with -c/--config)");
                        }
                    }
                    ControlCommand::Stats => {
                        out.event(&format!(
                            "  ▶ CONTROL stats t={:.1}s mid={:.4} regime={} active={} paused={} throughput={}x",
                            current_time,
                            mid,
                            state.current,
                            active_orders.len(),
                            runtime.paused,
                            runtime.throughput_scale
                        ));
                    }
                    _ => out.event("  ⚠ invalid control value"),
                }
            }
        }

        if runtime.paused {
            std::thread::sleep(Duration::from_secs_f64(cfg.tick_interval));
            continue;
        }

        // --- Forced scenario event ---
        if !forced_event_fired
            && scenario_cfg.forced_event_time > 0.0
            && current_time >= scenario_cfg.forced_event_time
        {
            forced_event_fired = true;
            state.transition_to(scenario_cfg.forced_regime, &mut rng);

            // Flash crash: short duration override
            if cfg.scenario == Scenario::FlashCrash {
                state.regime_duration = 3.0 + rng.gen::<f64>() * 4.0;
            }

            out.event(&format!(
                "  ▶ FORCED EVENT  regime -> {}  t={:.1}s",
                state.current, current_time
            ));
        }

        // --- Shock event ---
        if rng.gen::<f64>() < runtime.shock_prob {
            let shock_pct =
                cfg.shock_min_pct + rng.gen::<f64>() * (cfg.shock_max_pct - cfg.shock_min_pct);
            let direction: f64 = if rng.gen::<f64>() < 0.5 { 1.0 } else { -1.0 };
            mid *= 1.0 + direction * shock_pct;
            mid = mid.max(cfg.tick_size);

            let sign = if direction > 0.0 { "+" } else { "" };
            out.event(&format!(
                "  ⚡ SHOCK  {}{:.2}% -> mid={:.4}  t={:.1}s",
                sign,
                shock_pct * 100.0 * direction,
                mid,
                current_time
            ));

            if state.current == Regime::Calm || state.current == Regime::Recovery {
                let next = if direction < 0.0 {
                    Regime::Crash
                } else {
                    Regime::Rally
                };
                state.transition_to(next, &mut rng);
                out.event(&format!(
                    "  ⚡ SHOCK triggered regime -> {}",
                    state.current
                ));
            }
        }

        let params = regime::params(state.current);

        // --- GBM mid-price update (mu is per-second, sigma is annualized) ---
        let drift_term = params.mu * dt_seconds;
        let z: f64 = {
            let u1: f64 = rng.gen::<f64>().max(1e-15);
            let u2: f64 = rng.gen();
            (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
        };
        let diffusion_term = params.sigma * dt.sqrt() * z;
        mid *= (drift_term + diffusion_term).exp();
        mid = mid.max(cfg.tick_size);

        // --- Print regime changes ---
        if state.current != last_printed_regime {
            let p = regime::params(state.current);
            out.event(&format!(
                "  ↔ REGIME  {} -> {}  (σ={} μ={} buy_prob={})  t={:.1}s",
                last_printed_regime, state.current, p.sigma, p.mu, p.buy_prob, current_time
            ));
            last_printed_regime = state.current;
        }

        // --- Generate orders for this tick (with throughput scaling) ---
        let mut tick_orders: Vec<Order> = Vec::new();

        let limit_lambda = params.limit_rate * runtime.throughput_scale * dt_seconds;
        let num_limits: u64 = if limit_lambda > 0.0 {
            rng.sample(Poisson::new(limit_lambda).unwrap()) as u64
        } else {
            0
        };

        let offset_dist = Exp::new(params.offset_lambda).unwrap();

        for _ in 0..num_limits {
            let side = if rng.gen::<f64>() < params.buy_prob {
                Side::Buy
            } else {
                Side::Sell
            };
            let offset = params.half_spread + rng.sample::<f64, _>(offset_dist);
            let raw_price = match side {
                Side::Buy => mid - offset,
                Side::Sell => mid + offset,
            };
            let price = (raw_price / cfg.tick_size).round() * cfg.tick_size;
            let size = (rng.sample::<f64, _>(size_dist).round() as u32).max(1);

            tick_orders.push(Order {
                id: next_id,
                side,
                order_type: OrderType::Limit,
                price,
                size,
                created_at: current_time,
                ttl: rng.sample(ttl_dist),
            });
            next_id += 1;
        }
        stats.limits_generated += num_limits;

        let market_lambda = params.market_rate * runtime.throughput_scale * dt_seconds;
        let num_markets: u64 = if market_lambda > 0.0 {
            rng.sample(Poisson::new(market_lambda).unwrap()) as u64
        } else {
            0
        };

        for _ in 0..num_markets {
            let side = if rng.gen::<f64>() < params.buy_prob {
                Side::Buy
            } else {
                Side::Sell
            };
            let price = match side {
                Side::Buy => 999_999.0,
                Side::Sell => 0.0,
            };
            let raw_size = rng.sample::<f64, _>(size_dist) * 0.5 * params.size_mult;
            let size = (raw_size.round() as u32).max(1);

            tick_orders.push(Order {
                id: next_id,
                side,
                order_type: OrderType::Market,
                price,
                size,
                created_at: current_time,
                ttl: 0.0,
            });
            next_id += 1;
        }
        stats.markets_generated += num_markets;

        tick_orders.shuffle(&mut rng);

        // --- Send orders ---
        for order in &tick_orders {
            let _ = sender.send_order(order);
            stats.messages_sent += 1;
            if order.order_type == OrderType::Limit {
                active_orders.insert(order.id, order.clone());
            }
        }

        // --- Cancel expired orders ---
        let expired: Vec<u64> = active_orders
            .iter()
            .filter(|(_, o)| o.ttl > 0.0 && (current_time - o.created_at) >= o.ttl)
            .map(|(&id, _)| id)
            .collect();

        for id in &expired {
            let _ = sender.send_cancel(*id, current_time);
            active_orders.remove(id);
            stats.messages_sent += 1;
        }
        stats.cancels_expired += expired.len() as u64;

        // --- Regime-driven cancellations (with throughput scaling) ---
        let cancel_lambda = params.cancel_rate * runtime.throughput_scale * dt_seconds;
        let num_cancels: u64 = if cancel_lambda > 0.0 {
            rng.sample(Poisson::new(cancel_lambda).unwrap()) as u64
        } else {
            0
        };

        if num_cancels > 0 && !active_orders.is_empty() {
            let count = num_cancels.min(active_orders.len() as u64);
            for _ in 0..count {
                if active_orders.is_empty() {
                    break;
                }
                let keys: Vec<u64> = active_orders.keys().copied().collect();
                let &pick = keys.choose(&mut rng).unwrap();
                let _ = sender.send_cancel(pick, current_time);
                active_orders.remove(&pick);
                stats.messages_sent += 1;
                stats.cancels_regime += 1;
            }
        }

        // --- Periodic display ---
        time_since_display += dt_seconds;
        if time_since_display >= runtime.display_interval {
            out.summary(
                current_time,
                mid,
                state.current,
                active_orders.len(),
                &stats,
                time_since_display,
            );
            stats.reset();
            time_since_display = 0.0;
        }

        // --- Regime transition ---
        state.time_in_regime += dt_seconds;
        let next = regime::try_transition(&state, scenario_cfg.allow_transitions, &mut rng);
        if next != state.current {
            state.transition_to(next, &mut rng);
        }

        current_time += dt_seconds;
        std::thread::sleep(Duration::from_secs_f64(dt_seconds));
    }

    out.event("Shutting down...");
    Ok(())
}
