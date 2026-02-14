use rand::Rng;
use serde::Deserialize;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Regime {
    Calm,
    Volatile,
    Crash,
    Rally,
    Recovery,
}

impl Regime {
    pub const ALL: [Regime; 5] = [
        Regime::Calm,
        Regime::Volatile,
        Regime::Crash,
        Regime::Rally,
        Regime::Recovery,
    ];

    pub fn index(self) -> usize {
        match self {
            Regime::Calm => 0,
            Regime::Volatile => 1,
            Regime::Crash => 2,
            Regime::Rally => 3,
            Regime::Recovery => 4,
        }
    }
}

impl fmt::Display for Regime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Regime::Calm => write!(f, "CALM"),
            Regime::Volatile => write!(f, "VOLATILE"),
            Regime::Crash => write!(f, "CRASH"),
            Regime::Rally => write!(f, "RALLY"),
            Regime::Recovery => write!(f, "RECOVERY"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RegimeParams {
    pub sigma: f64,
    pub mu: f64, // per-second drift rate (not annualized)
    pub limit_rate: f64,
    pub market_rate: f64,
    pub cancel_rate: f64,
    pub buy_prob: f64,
    pub half_spread: f64,
    pub offset_lambda: f64,
    pub size_mult: f64,
    pub min_duration: f64,
    pub max_duration: f64,
}

pub const REGIME_TABLE: [RegimeParams; 5] = [
    // CALM
    RegimeParams {
        sigma: 0.15, mu: 0.0, limit_rate: 50.0, market_rate: 5.0, cancel_rate: 20.0,
        buy_prob: 0.50, half_spread: 0.03, offset_lambda: 5.0, size_mult: 1.0,
        min_duration: 5.0, max_duration: 30.0,
    },
    // VOLATILE
    RegimeParams {
        sigma: 0.80, mu: 0.0, limit_rate: 80.0, market_rate: 15.0, cancel_rate: 40.0,
        buy_prob: 0.50, half_spread: 0.08, offset_lambda: 2.5, size_mult: 1.5,
        min_duration: 3.0, max_duration: 15.0,
    },
    // CRASH — mu=-0.045/s → exp(-0.045*5) ≈ 0.80, so ~100→80 over 5s
    RegimeParams {
        sigma: 2.00, mu: -0.045, limit_rate: 15.0, market_rate: 45.0, cancel_rate: 80.0,
        buy_prob: 0.12, half_spread: 0.25, offset_lambda: 1.2, size_mult: 3.0,
        min_duration: 2.0, max_duration: 10.0,
    },
    // RALLY — mu=+0.035/s → exp(0.035*5) ≈ 1.19, so ~100→119 over 5s
    RegimeParams {
        sigma: 1.50, mu: 0.035, limit_rate: 25.0, market_rate: 35.0, cancel_rate: 50.0,
        buy_prob: 0.88, half_spread: 0.15, offset_lambda: 1.8, size_mult: 2.5,
        min_duration: 2.0, max_duration: 12.0,
    },
    // RECOVERY — mu=+0.005/s → gentle upward drift
    RegimeParams {
        sigma: 0.50, mu: 0.005, limit_rate: 60.0, market_rate: 8.0, cancel_rate: 25.0,
        buy_prob: 0.55, half_spread: 0.05, offset_lambda: 4.0, size_mult: 1.0,
        min_duration: 3.0, max_duration: 15.0,
    },
];

/// Markov transition probabilities per tick.
/// Rows = from regime, columns = to regime.
/// Order: CALM, VOLATILE, CRASH, RALLY, RECOVERY
pub const TRANSITION_PROB: [[f64; 5]; 5] = [
    /* CALM     */ [0.0,   0.008, 0.003, 0.003, 0.0  ],
    /* VOLATILE */ [0.005, 0.0,   0.008, 0.006, 0.004],
    /* CRASH    */ [0.0,   0.004, 0.0,   0.002, 0.020],
    /* RALLY    */ [0.0,   0.006, 0.002, 0.0,   0.015],
    /* RECOVERY */ [0.015, 0.004, 0.001, 0.002, 0.0  ],
];

pub struct RegimeState {
    pub current: Regime,
    pub time_in_regime: f64,
    pub regime_duration: f64,
    pub previous: Regime,
}

impl RegimeState {
    pub fn new(regime: Regime, rng: &mut impl Rng) -> Self {
        Self {
            current: regime,
            time_in_regime: 0.0,
            regime_duration: random_regime_duration(regime, rng),
            previous: regime,
        }
    }

    pub fn transition_to(&mut self, next: Regime, rng: &mut impl Rng) {
        self.previous = self.current;
        self.current = next;
        self.time_in_regime = 0.0;
        self.regime_duration = random_regime_duration(next, rng);
    }
}

pub fn params(regime: Regime) -> &'static RegimeParams {
    &REGIME_TABLE[regime.index()]
}

pub fn try_transition(state: &RegimeState, allow_transitions: bool, rng: &mut impl Rng) -> Regime {
    if !allow_transitions {
        return state.current;
    }
    if state.time_in_regime < state.regime_duration {
        return state.current;
    }

    let from = state.current.index();
    let roll: f64 = rng.gen();
    let mut cumulative = 0.0;

    for (to, &prob) in TRANSITION_PROB[from].iter().enumerate() {
        cumulative += prob;
        if roll < cumulative {
            return Regime::ALL[to];
        }
    }

    state.current
}

pub fn random_regime_duration(regime: Regime, rng: &mut impl Rng) -> f64 {
    let p = params(regime);
    rng.gen_range(p.min_duration..=p.max_duration)
}
