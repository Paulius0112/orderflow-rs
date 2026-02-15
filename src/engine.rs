use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;
use rand_distr::{Exp, LogNormal, Poisson, Uniform};
use std::collections::HashMap;

use crate::config::AppConfig;
use crate::multicast::MulticastSender;
use crate::order::{Order, OrderType, Side};
use crate::regime::{self, Regime, RegimeState};
use crate::scenario::{Scenario, ScenarioConfig};

/// GBM dt: tick interval expressed in years.
fn dt_years(tick_interval: f64) -> f64 {
    1.0 / (252.0 * 6.5 * 3600.0) * tick_interval / 0.1 * 0.1
}

pub fn run(cfg: &AppConfig) -> Result<(), Box<dyn std::error::Error>> {
    let mut rng = StdRng::seed_from_u64(cfg.seed);

    let scenario_cfg = ScenarioConfig::from_scenario(cfg.scenario);
    let mut state = RegimeState::new(scenario_cfg.starting_regime, &mut rng);
    let mut forced_event_fired = false;

    let sender = MulticastSender::new(cfg.multicast_group, cfg.multicast_port)?;

    println!("Starting Order Generation Engine");
    println!("Scenario: {}", cfg.scenario);
    println!("Seed: {}", cfg.seed);
    println!("Initial regime: {}", state.current);

    let dt = dt_years(cfg.tick_interval);
    let dt_seconds = cfg.tick_interval;

    let size_dist = LogNormal::new(cfg.size_mean_log, cfg.size_std_log)?;
    let ttl_dist = Uniform::new(cfg.ttl_min, cfg.ttl_max);

    let mut mid = cfg.initial_price;
    let mut next_id: u64 = 0;
    let mut active_orders: HashMap<u64, Order> = HashMap::new();
    let mut current_time: f64 = 0.0;
    let mut last_printed_regime = state.current;

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

            println!(">>> FORCED EVENT: regime -> {}", state.current);
        }

        // --- Shock event ---
        if rng.gen::<f64>() < cfg.shock_prob {
            let shock_pct =
                cfg.shock_min_pct + rng.gen::<f64>() * (cfg.shock_max_pct - cfg.shock_min_pct);
            let direction: f64 = if rng.gen::<f64>() < 0.5 { 1.0 } else { -1.0 };
            mid *= 1.0 + direction * shock_pct;
            mid = mid.max(cfg.tick_size);

            let sign = if direction > 0.0 { "+" } else { "" };
            println!(
                ">>> SHOCK: {}{:.2}% -> mid={:.4}",
                sign,
                shock_pct * 100.0 * direction,
                mid
            );

            if state.current == Regime::Calm || state.current == Regime::Recovery {
                let next = if direction < 0.0 {
                    Regime::Crash
                } else {
                    Regime::Rally
                };
                state.transition_to(next, &mut rng);
                println!(">>> SHOCK triggered regime -> {}", state.current);
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
            println!(
                "--- Regime: {} (sigma={} mu={} buy_prob={}) ---",
                state.current, p.sigma, p.mu, p.buy_prob
            );
            last_printed_regime = state.current;
        }

        println!("Mid: {:.4}", mid);

        // --- Generate orders for this tick ---
        let mut tick_orders: Vec<Order> = Vec::new();

        let limit_lambda = params.limit_rate * dt_seconds;
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

        let market_lambda = params.market_rate * dt_seconds;
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

        tick_orders.shuffle(&mut rng);

        // --- Send orders ---
        for order in &tick_orders {
            let _ = sender.send_order(order);
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

        for id in expired {
            let _ = sender.send_cancel(id, current_time);
            active_orders.remove(&id);
        }

        // --- Regime-driven cancellations ---
        let cancel_lambda = params.cancel_rate * dt_seconds;
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
            }
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