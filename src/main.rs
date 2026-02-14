mod config;
mod engine;
mod multicast;
mod order;
mod regime;
mod scenario;

use clap::Parser;
use config::{AppConfig, Cli};

fn main() {
    let cli = Cli::parse();

    let cfg = match AppConfig::resolve(&cli) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    };

    if let Err(e) = engine::run(&cfg) {
        eprintln!("fatal: {}", e);
        std::process::exit(1);
    }
}
