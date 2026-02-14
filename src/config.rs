use clap::Parser;
use serde::Deserialize;
use std::net::Ipv4Addr;
use std::path::PathBuf;

use crate::scenario::Scenario;

/// Market microstructure simulator for stress-testing order books.
#[derive(Debug, Parser)]
#[command(name = "orderflow-rs")]
#[command(about = "Realistic order generation engine with regime-based market dynamics")]
pub struct Cli {
    /// Market scenario to simulate
    #[arg(long, value_name = "SCENARIO")]
    pub scenario: Option<String>,

    /// Path to TOML configuration file
    #[arg(short, long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Multicast group address
    #[arg(long, value_name = "ADDR")]
    pub multicast_group: Option<String>,

    /// Multicast port
    #[arg(long, value_name = "PORT")]
    pub multicast_port: Option<u16>,

    /// Initial mid-price
    #[arg(long, value_name = "PRICE")]
    pub initial_price: Option<f64>,

    /// Tick interval in seconds
    #[arg(long, value_name = "SECONDS")]
    pub tick_interval: Option<f64>,

    /// Minimum price increment
    #[arg(long, value_name = "SIZE")]
    pub tick_size: Option<f64>,

    /// Shock probability per tick
    #[arg(long, value_name = "PROB")]
    pub shock_prob: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct FileConfig {
    #[serde(default)]
    pub simulation: SimulationConfig,

    #[serde(default)]
    pub network: NetworkConfig,

    #[serde(default)]
    pub orders: OrderConfig,

    #[serde(default)]
    pub shocks: ShockConfig,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct SimulationConfig {
    pub scenario: Scenario,
    pub initial_price: f64,
    pub tick_interval: f64,
    pub tick_size: f64,
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            scenario: Scenario::Normal,
            initial_price: 100.0,
            tick_interval: 0.1,
            tick_size: 0.01,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct NetworkConfig {
    pub multicast_group: String,
    pub multicast_port: u16,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            multicast_group: "239.255.0.1".to_string(),
            multicast_port: 5555,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct OrderConfig {
    pub size_mean_log: f64,
    pub size_std_log: f64,
    pub ttl_min: f64,
    pub ttl_max: f64,
}

impl Default for OrderConfig {
    fn default() -> Self {
        Self {
            size_mean_log: 3.0,
            size_std_log: 1.0,
            ttl_min: 1.0,
            ttl_max: 30.0,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct ShockConfig {
    pub probability: f64,
    pub min_pct: f64,
    pub max_pct: f64,
}

impl Default for ShockConfig {
    fn default() -> Self {
        Self {
            probability: 0.0003,
            min_pct: 0.02,
            max_pct: 0.06,
        }
    }
}

impl Default for FileConfig {
    fn default() -> Self {
        Self {
            simulation: SimulationConfig::default(),
            network: NetworkConfig::default(),
            orders: OrderConfig::default(),
            shocks: ShockConfig::default(),
        }
    }
}

/// Resolved configuration after merging TOML file + CLI overrides.
pub struct AppConfig {
    pub scenario: Scenario,
    pub initial_price: f64,
    pub tick_interval: f64,
    pub tick_size: f64,
    pub multicast_group: Ipv4Addr,
    pub multicast_port: u16,
    pub size_mean_log: f64,
    pub size_std_log: f64,
    pub ttl_min: f64,
    pub ttl_max: f64,
    pub shock_prob: f64,
    pub shock_min_pct: f64,
    pub shock_max_pct: f64,
}

impl AppConfig {
    /// Build the final config: TOML defaults -> file values -> CLI overrides.
    pub fn resolve(cli: &Cli) -> Result<Self, Box<dyn std::error::Error>> {
        let mut file_cfg = if let Some(path) = &cli.config {
            let contents = std::fs::read_to_string(path)
                .map_err(|e| format!("failed to read config file {}: {}", path.display(), e))?;
            toml::from_str::<FileConfig>(&contents)
                .map_err(|e| format!("failed to parse config file: {}", e))?
        } else {
            FileConfig::default()
        };

        // CLI overrides
        if let Some(s) = &cli.scenario {
            file_cfg.simulation.scenario = parse_scenario(s)?;
        }
        if let Some(v) = cli.initial_price {
            file_cfg.simulation.initial_price = v;
        }
        if let Some(v) = cli.tick_interval {
            file_cfg.simulation.tick_interval = v;
        }
        if let Some(v) = cli.tick_size {
            file_cfg.simulation.tick_size = v;
        }
        if let Some(g) = &cli.multicast_group {
            file_cfg.network.multicast_group = g.clone();
        }
        if let Some(p) = cli.multicast_port {
            file_cfg.network.multicast_port = p;
        }
        if let Some(v) = cli.shock_prob {
            file_cfg.shocks.probability = v;
        }

        let multicast_group: Ipv4Addr = file_cfg
            .network
            .multicast_group
            .parse()
            .map_err(|e| format!("invalid multicast group '{}': {}", file_cfg.network.multicast_group, e))?;

        Ok(Self {
            scenario: file_cfg.simulation.scenario,
            initial_price: file_cfg.simulation.initial_price,
            tick_interval: file_cfg.simulation.tick_interval,
            tick_size: file_cfg.simulation.tick_size,
            multicast_group,
            multicast_port: file_cfg.network.multicast_port,
            size_mean_log: file_cfg.orders.size_mean_log,
            size_std_log: file_cfg.orders.size_std_log,
            ttl_min: file_cfg.orders.ttl_min,
            ttl_max: file_cfg.orders.ttl_max,
            shock_prob: file_cfg.shocks.probability,
            shock_min_pct: file_cfg.shocks.min_pct,
            shock_max_pct: file_cfg.shocks.max_pct,
        })
    }
}

fn parse_scenario(s: &str) -> Result<Scenario, Box<dyn std::error::Error>> {
    match s {
        "normal" => Ok(Scenario::Normal),
        "crash" => Ok(Scenario::Crash),
        "volatile" => Ok(Scenario::Volatile),
        "flash-crash" => Ok(Scenario::FlashCrash),
        "rally" => Ok(Scenario::Rally),
        _ => Err(format!(
            "unknown scenario '{}'. available: normal, crash, volatile, flash-crash, rally",
            s
        )
        .into()),
    }
}
