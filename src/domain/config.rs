use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AccumulationMode {
    #[default]
    Single,
    Accumulate,
}

impl std::fmt::Display for AccumulationMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AccumulationMode::Single => write!(f, "single"),
            AccumulationMode::Accumulate => write!(f, "accumulate"),
        }
    }
}

impl std::str::FromStr for AccumulationMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.eq_ignore_ascii_case("single") {
            Ok(AccumulationMode::Single)
        } else if s.eq_ignore_ascii_case("accumulate") {
            Ok(AccumulationMode::Accumulate)
        } else {
            Err(format!("invalid accumulation mode: {}", s))
        }
    }
}

/// One named experiment configuration, registered via `dstest.config()` and
/// referenced by handle in `dstest.setup()` / `dstest.dst.step()`.
#[derive(Clone, Debug)]
pub struct Config {
    pub substrate: Option<String>,
    pub seed: Option<u64>,
    /// Fault weights. BTreeMap so iteration order — and therefore the
    /// weighted-selection order — is deterministic for a given seed.
    pub fault_weights: BTreeMap<String, f32>,
    pub accumulation_mode: AccumulationMode,
    pub http_timeout_secs: u64,
    pub http_retries: u32,
    pub http_retry_delay_ms: u64,
    pub step_delay_ms: u64,
    /// Total number of fault steps the fault tree will produce.
    pub steps: usize,
    pub require_seed: bool,
}

impl Default for Config {
    fn default() -> Self {
        let mut fault_weights = BTreeMap::new();
        fault_weights.insert(String::from("pause"), 0.35);
        fault_weights.insert(String::from("kill"), 0.25);
        fault_weights.insert(String::from("deprive:disk"), 0.10);
        fault_weights.insert(String::from("deprive:network"), 0.10);
        fault_weights.insert(String::from("deprive:memory"), 0.10);
        fault_weights.insert(String::from("deprive:cpu"), 0.10);

        Self {
            substrate: None,
            seed: None,
            fault_weights,
            accumulation_mode: AccumulationMode::Single,
            http_timeout_secs: 5,
            http_retries: 30,
            http_retry_delay_ms: 500,
            step_delay_ms: 1000,
            steps: 10,
            require_seed: true,
        }
    }
}

impl Config {
    /// Validate invariants that normalization cannot fix. Call *before*
    /// [`Config::normalize_weights`].
    pub fn validate(&self) -> Result<(), String> {
        for (name, weight) in &self.fault_weights {
            if *weight < 0.0 {
                return Err(format!("weight for '{}' is negative: {}", name, weight));
            }
        }

        Ok(())
    }

    pub fn normalize_weights(&mut self) {
        let total: f32 = self.fault_weights.values().sum();
        if total > 0.0 {
            for weight in self.fault_weights.values_mut() {
                *weight /= total;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_accumulation_mode_display() {
        assert_eq!(AccumulationMode::Single.to_string(), "single");
        assert_eq!(AccumulationMode::Accumulate.to_string(), "accumulate");
    }

    #[test]
    fn test_accumulation_mode_from_str() {
        assert!(matches!(
            AccumulationMode::from_str("single"),
            Ok(AccumulationMode::Single)
        ));
        assert!(matches!(
            AccumulationMode::from_str("SINGLE"),
            Ok(AccumulationMode::Single)
        ));
        assert!(matches!(
            AccumulationMode::from_str("accumulate"),
            Ok(AccumulationMode::Accumulate)
        ));
        assert!(AccumulationMode::from_str("invalid").is_err());
    }

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert!(config.fault_weights.contains_key("pause"));
        assert!(config.fault_weights.contains_key("kill"));
        assert!(config.require_seed);
        assert_eq!(config.steps, 10);
    }

    #[test]
    fn test_config_validate() {
        let config = Config::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validate_negative_weight() {
        let mut config = Config::default();
        config.fault_weights.insert("test".to_string(), -0.5);
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_normalize_weights() {
        let mut config = Config::default();
        config.fault_weights.insert("extra".to_string(), 1.0);
        config.normalize_weights();
        let total: f32 = config.fault_weights.values().sum();
        assert!((total - 1.0).abs() < 0.01);
    }
}
