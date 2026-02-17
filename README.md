# Trading Engine Orders

A realistic order generation engine for stress-testing order books. Simulates market microstructure with regime-based dynamics — prices don't just random walk, they crash, rally, and recover like real markets.

## Build

```bash
cargo build --release
```

## Usage

```bash
# Default (normal scenario)
./target/release/trading-engine-orders

# Select a scenario via CLI
./target/release/trading-engine-orders --scenario crash

# Use a TOML config file
./target/release/trading-engine-orders -c config.toml

# Config file + CLI overrides (CLI wins)
./target/release/trading-engine-orders -c config.toml --scenario rally --initial-price 250.0
```

### CLI Options

| Flag | Description |
|------|-------------|
| `--scenario <NAME>` | Market scenario: `normal`, `crash`, `volatile`, `flash-crash`, `rally` |
| `-c, --config <FILE>` | Path to TOML configuration file |
| `--multicast-group <ADDR>` | UDP multicast group (default: `239.255.0.1`) |
| `--multicast-port <PORT>` | UDP multicast port (default: `5555`) |
| `--initial-price <PRICE>` | Starting mid-price (default: `100.0`) |
| `--tick-interval <SECS>` | Tick interval in seconds (default: `0.1`) |
| `--tick-size <SIZE>` | Minimum price increment (default: `0.01`) |
| `--shock-prob <PROB>` | Shock probability per tick (default: `0.0003`) |
| `--wire-format <FORMAT>` | Network wire format: `text`, `binary` |
| `--control-enabled <BOOL>` | Enable runtime UDP control API |
| `--control-bind <ADDR:PORT>` | Control API bind address (default: `127.0.0.1:6001`) |

### Configuration File

See [`config.toml`](config.toml) for all available settings. CLI flags override values from the config file.

## Scenarios

| Scenario | Description |
|----------|-------------|
| `normal` | Starts calm, naturally transitions through all regimes via Markov chain. Crashes and rallies happen organically. |
| `crash` | 10 seconds of calm trading, then a forced crash with negative drift and sell-heavy order flow. |
| `rally` | 10 seconds of calm trading, then a forced rally with positive drift and buy-heavy order flow. |
| `flash-crash` | 8 seconds of calm, then a short (3-7s) crash followed by rapid recovery. |
| `volatile` | Sustained high volatility with no regime transitions. Pure throughput stress testing. |

## Market Regimes

The simulator uses a state machine with 5 regimes. Each regime controls volatility, drift, order rates, buy/sell bias, spread width, book depth, and cancellation behavior.

| Regime | Sigma | Drift | Limit/s | Market/s | Buy Bias | Spread |
|--------|-------|-------|---------|----------|----------|--------|
| CALM | 0.15 | 0.0 | 50 | 5 | 50% | 0.03 |
| VOLATILE | 0.80 | 0.0 | 80 | 15 | 50% | 0.08 |
| CRASH | 2.00 | -1.50 | 15 | 45 | 12% | 0.25 |
| RALLY | 1.50 | +1.20 | 25 | 35 | 88% | 0.15 |
| RECOVERY | 0.50 | +0.30 | 60 | 8 | 55% | 0.05 |

## How It Works

**Price Model** — Geometric Brownian Motion with regime-dependent drift and volatility.

**Regime Transitions** — Markov chain with per-tick transition probabilities. Typical flow: `CALM -> VOLATILE -> CRASH -> RECOVERY -> CALM`.

**Shock Events** — Rare (~once per 5 min), sudden 2-6% price jumps that trigger immediate regime changes when the market is calm.

**Order Generation** — Each tick (100ms): limit orders arrive at Poisson rates with exponential offsets from mid; market orders cross the book; expired and regime-driven cancellations remove liquidity.

## Wire Protocol

Orders are sent via UDP multicast with selectable format.

### Text format (`wire_format = "text"`)

```
ORDER|id=42|side=BUY|type=LIMIT|price=99.85|size=23|time=1.300
CANCEL|id=42|time=5.700
```

### Binary format (`wire_format = "binary"`)

Little-endian frames with header:

- `magic[2] = "OF"`
- `version = 1`
- `msg_type = 1` for ORDER, `2` for CANCEL

ORDER payload:

- `id:u64`
- `side:u8` (`1=BUY`, `2=SELL`)
- `order_type:u8` (`1=LIMIT`, `2=MARKET`)
- `price:f64`
- `size:u32`
- `time:f64`

CANCEL payload:

- `id:u64`
- `time:f64`

## Runtime Control API

When `[control].enabled = true`, the engine listens on UDP (default `127.0.0.1:6001`) for live commands:

- `pause`
- `resume`
- `rate <multiplier>` (example: `rate 4.0`)
- `display <seconds>` (example: `display 0.5`)
- `regime <calm|volatile|crash|rally|recovery>`
- `reload` (reloads runtime tunables from `-c/--config`)
- `stats`

Example:

```bash
echo "rate 10.0" | nc -u -w1 127.0.0.1 6001
echo "regime crash" | nc -u -w1 127.0.0.1 6001
```
# orderflow-rs
