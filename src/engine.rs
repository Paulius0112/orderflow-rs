use rand::seq::SliceRandom;
use rand::Rng;
use rand_distr::{Exp, LogNormal, Poisson, Uniform};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;

use crate::config::{AppConfig, OutputMode};
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
    let mut rng = rand::thread_rng();

    let scenario_cfg = ScenarioConfig::from_scenario(cfg.scenario);
    let mut state = RegimeState::new(scenario_cfg.starting_regime, &mut rng);
    let mut forced_event_fired = false;

    let sender = MulticastSender::new(cfg.multicast_group, cfg.multicast_port)?;
    let mut out = Output::new(cfg)?;

    let scale = cfg.throughput_scale;

    // --- Startup banner ---
    out.print(&box_top());
    out.print(&box_line("Order Generation Engine"));
    out.print(&box_mid());
    out.print(&box_line(&format!("scenario:    {}", cfg.scenario)));
    out.print(&box_line(&format!("regime:      {}", state.current)));
    out.print(&box_line(&format!("mid price:   {}", cfg.initial_price)));
    out.print(&box_line(&format!("tick:        {}s", cfg.tick_interval)));
    out.print(&box_line(&format!("throughput:  {}x", scale)));
    out.print(&box_line(&format!("output:      {}", cfg.output_mode)));
    if out.to_file() {
        out.print(&box_line(&format!("log file:    {}", cfg.log_file)));
    }
    out.print(&box_line(&format!(
        "multicast:   {}:{}",
        cfg.multicast_group, cfg.multicast_port
    )));
    out.print(&box_bottom());

    let dt = dt_years(cfg.tick_interval);
    let dt_seconds = cfg.tick_interval;
    let display_interval = cfg.display_interval;

    let size_dist = LogNormal::new(cfg.size_mean_log, cfg.size_std_log)?;
    let ttl_dist = Uniform::new(cfg.ttl_min, cfg.ttl_max);

    let mut mid = cfg.initial_price;
    let mut next_id: u64 = 0;
    let mut active_orders: HashMap<u64, Order> = HashMap::new();
    let mut current_time: f64 = 0.0;
    let mut last_printed_regime = state.current;

    let mut stats = TickStats::new();
    let mut time_since_display: f64 = 0.0;

    loop {
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
        if rng.gen::<f64>() < cfg.shock_prob {
            let shock_pct =
                cfg.shock_min_pct + rng.gen::<f64>() * (cfg.shock_max_pct - cfg.shock_min_pct);
            let direction: f64 = if rng.gen::<f64>() < 0.5 { 1.0 } else { -1.0 };
            mid *= 1.0 + direction * shock_pct;

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

        let limit_lambda = params.limit_rate * scale * dt_seconds;
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

        let market_lambda = params.market_rate * scale * dt_seconds;
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
        let cancel_lambda = params.cancel_rate * scale * dt_seconds;
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
        if time_since_display >= display_interval {
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
        std::thread::sleep(std::time::Duration::from_secs_f64(dt_seconds));
    }
}
