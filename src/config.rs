use clap::Parser;
use serde::Deserialize;
use std::fmt;
use std::net::Ipv4Addr;
use std::path::PathBuf;

use crate::scenario::Scenario;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputMode {
    Console,
    File,
    Both,
    Quiet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WireFormat {
    Text,
    Binary,
}

impl Default for WireFormat {
    fn default() -> Self {
        Self::Text
    }
}

impl fmt::Display for WireFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WireFormat::Text => write!(f, "text"),
            WireFormat::Binary => write!(f, "binary"),
        }
    }
}

impl Default for OutputMode {
    fn default() -> Self {
        Self::Console
    }
}

impl fmt::Display for OutputMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OutputMode::Console => write!(f, "console"),
            OutputMode::File => write!(f, "file"),
            OutputMode::Both => write!(f, "both"),
            OutputMode::Quiet => write!(f, "quiet"),
        }
    }
}

fn parse_output_mode(s: &str) -> Result<OutputMode, Box<dyn std::error::Error>> {
    match s {
        "console" => Ok(OutputMode::Console),
        "file" => Ok(OutputMode::File),
        "both" => Ok(OutputMode::Both),
        "quiet" => Ok(OutputMode::Quiet),
        _ => Err(format!(
            "unknown output mode '{}'. available: console, file, both, quiet",
            s
        )
        .into()),
    }
}

fn parse_wire_format(s: &str) -> Result<WireFormat, Box<dyn std::error::Error>> {
    match s {
        "text" => Ok(WireFormat::Text),
        "binary" => Ok(WireFormat::Binary),
        _ => Err(format!("unknown wire format '{}'. available: text, binary", s).into()),
    }
}

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

    /// Output mode: console, file, both, quiet
    #[arg(long, value_name = "MODE")]
    pub output: Option<String>,

    /// Log file path (used when output mode is file or both)
    #[arg(long, value_name = "PATH")]
    pub log_file: Option<String>,

    /// Throughput multiplier applied to order generation rates (default: 1.0)
    #[arg(long, value_name = "SCALE")]
    pub throughput_scale: Option<f64>,

    /// Console display interval in seconds (how often stats are printed)
    #[arg(long, value_name = "SECONDS")]
    pub display_interval: Option<f64>,

    /// RNG seed for reproducible runs (random if omitted)
    #[arg(long, value_name = "SEED")]
    pub seed: Option<u64>,

    /// Wire format used on multicast: text, binary
    #[arg(long, value_name = "FORMAT")]
    pub wire_format: Option<String>,

    /// Enable UDP control API on localhost (pause/resume/rate/regime/reload)
    #[arg(long, value_name = "BOOL")]
    pub control_enabled: Option<bool>,

    /// UDP control API bind address (example: 127.0.0.1:6001)
    #[arg(long, value_name = "ADDR:PORT")]
    pub control_bind: Option<String>,
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

    #[serde(default)]
    pub output: OutputConfig,

    #[serde(default)]
    pub control: ControlConfig,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct SimulationConfig {
    pub scenario: Scenario,
    pub initial_price: f64,
    pub tick_interval: f64,
    pub tick_size: f64,
    pub throughput_scale: f64,
    pub seed: Option<u64>,
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            scenario: Scenario::Normal,
            initial_price: 100.0,
            tick_interval: 0.1,
            tick_size: 0.01,
            throughput_scale: 1.0,
            seed: None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct OutputConfig {
    pub mode: OutputMode,
    pub log_file: String,
    pub display_interval: f64,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            mode: OutputMode::Console,
            log_file: "orderflow.log".to_string(),
            display_interval: 1.0,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct NetworkConfig {
    pub multicast_group: String,
    pub multicast_port: u16,
    pub wire_format: WireFormat,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            multicast_group: "239.255.0.1".to_string(),
            multicast_port: 5555,
            wire_format: WireFormat::Text,
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

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct ControlConfig {
    pub enabled: bool,
    pub bind: String,
}

impl Default for ControlConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            bind: "127.0.0.1:6001".to_string(),
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
            output: OutputConfig::default(),
            control: ControlConfig::default(),
        }
    }
}

/// Resolved configuration after merging TOML file + CLI overrides.
pub struct AppConfig {
    pub config_path: Option<PathBuf>,
    pub scenario: Scenario,
    pub initial_price: f64,
    pub tick_interval: f64,
    pub tick_size: f64,
    pub multicast_group: Ipv4Addr,
    pub multicast_port: u16,
    pub wire_format: WireFormat,
    pub size_mean_log: f64,
    pub size_std_log: f64,
    pub ttl_min: f64,
    pub ttl_max: f64,
    pub shock_prob: f64,
    pub shock_min_pct: f64,
    pub shock_max_pct: f64,
    pub output_mode: OutputMode,
    pub log_file: String,
    pub display_interval: f64,
    pub throughput_scale: f64,
    pub seed: u64,
    pub control_enabled: bool,
    pub control_bind: String,
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
        if let Some(ref f) = cli.wire_format {
            file_cfg.network.wire_format = parse_wire_format(f)?;
        }
        if let Some(v) = cli.shock_prob {
            file_cfg.shocks.probability = v;
        }
        if let Some(ref m) = cli.output {
            file_cfg.output.mode = parse_output_mode(m)?;
        }
        if let Some(ref p) = cli.log_file {
            file_cfg.output.log_file = p.clone();
        }
        if let Some(v) = cli.throughput_scale {
            file_cfg.simulation.throughput_scale = v;
        }
        if let Some(v) = cli.display_interval {
            file_cfg.output.display_interval = v;
        }
        if let Some(v) = cli.seed {
            file_cfg.simulation.seed = Some(v);
        }
        if let Some(v) = cli.control_enabled {
            file_cfg.control.enabled = v;
        }
        if let Some(ref v) = cli.control_bind {
            file_cfg.control.bind = v.clone();
        }

        let seed = file_cfg
            .simulation
            .seed
            .unwrap_or_else(|| rand::random());

        let multicast_group: Ipv4Addr = file_cfg
            .network
            .multicast_group
            .parse()
            .map_err(|e| format!("invalid multicast group '{}': {}", file_cfg.network.multicast_group, e))?;

        Ok(Self {
            config_path: cli.config.clone(),
            scenario: file_cfg.simulation.scenario,
            initial_price: file_cfg.simulation.initial_price,
            tick_interval: file_cfg.simulation.tick_interval,
            tick_size: file_cfg.simulation.tick_size,
            multicast_group,
            multicast_port: file_cfg.network.multicast_port,
            wire_format: file_cfg.network.wire_format,
            size_mean_log: file_cfg.orders.size_mean_log,
            size_std_log: file_cfg.orders.size_std_log,
            ttl_min: file_cfg.orders.ttl_min,
            ttl_max: file_cfg.orders.ttl_max,
            shock_prob: file_cfg.shocks.probability,
            shock_min_pct: file_cfg.shocks.min_pct,
            shock_max_pct: file_cfg.shocks.max_pct,
            output_mode: file_cfg.output.mode,
            log_file: file_cfg.output.log_file,
            display_interval: file_cfg.output.display_interval,
            throughput_scale: file_cfg.simulation.throughput_scale,
            seed,
            control_enabled: file_cfg.control.enabled,
            control_bind: file_cfg.control.bind,
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
