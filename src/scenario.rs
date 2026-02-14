use serde::Deserialize;
use std::fmt;

use crate::regime::Regime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scenario {
    Normal,
    Crash,
    Volatile,
    FlashCrash,
    Rally,
}

impl fmt::Display for Scenario {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Scenario::Normal => write!(f, "normal"),
            Scenario::Crash => write!(f, "crash"),
            Scenario::Volatile => write!(f, "volatile"),
            Scenario::FlashCrash => write!(f, "flash-crash"),
            Scenario::Rally => write!(f, "rally"),
        }
    }
}

pub struct ScenarioConfig {
    pub starting_regime: Regime,
    pub forced_event_time: f64,
    pub forced_regime: Regime,
    pub allow_transitions: bool,
}

impl ScenarioConfig {
    pub fn from_scenario(scenario: Scenario) -> Self {
        match scenario {
            Scenario::Normal => Self {
                starting_regime: Regime::Calm,
                forced_event_time: -1.0,
                forced_regime: Regime::Calm,
                allow_transitions: true,
            },
            Scenario::Crash => Self {
                starting_regime: Regime::Calm,
                forced_event_time: 10.0,
                forced_regime: Regime::Crash,
                allow_transitions: true,
            },
            Scenario::Volatile => Self {
                starting_regime: Regime::Volatile,
                forced_event_time: -1.0,
                forced_regime: Regime::Volatile,
                allow_transitions: false,
            },
            Scenario::FlashCrash => Self {
                starting_regime: Regime::Calm,
                forced_event_time: 8.0,
                forced_regime: Regime::Crash,
                allow_transitions: true,
            },
            Scenario::Rally => Self {
                starting_regime: Regime::Calm,
                forced_event_time: 10.0,
                forced_regime: Regime::Rally,
                allow_transitions: true,
            },
        }
    }
}
