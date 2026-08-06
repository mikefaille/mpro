use figment::{
    providers::{Env, Format, Serialized, Toml},
    Figment,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Thermal Control Configuration Parameters
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ThermalConfig {
    /// Hour when acoustic night sleep window begins (0-23, default: 23 for 11 PM)
    pub night_start_hour: u32,
    /// Hour when acoustic night sleep window ends (0-23, default: 11 for 11 AM)
    pub night_end_hour: u32,
    /// Target Northbridge thermal setpoint in °C (default: 54.0 °C)
    pub target_setpoint: f64,
    /// Critical QPI clock freeze limit in °C (default: 63.0 °C)
    pub critical_limit: f64,
}

impl Default for ThermalConfig {
    fn default() -> Self {
        Self {
            night_start_hour: 23,
            night_end_hour: 11,
            target_setpoint: 54.0,
            critical_limit: 63.0,
        }
    }
}

impl ThermalConfig {
    /// Loads configuration hierarchically using Figment:
    /// 1. Default fallback struct values
    /// 2. TOML Configuration file (`/etc/mpro.conf` or custom path)
    /// 3. Environment variables prefixed with `MPRO_` (e.g. `MPRO_NIGHT_START_HOUR=23`)
    pub fn load(config_path: Option<&str>) -> Self {
        let mut figment = Figment::from(Serialized::defaults(Self::default()));

        let path_str = config_path.unwrap_or("/etc/mpro.conf");
        if Path::new(path_str).exists() {
            figment = figment.merge(Toml::file(path_str));
        }

        figment = figment.merge(Env::prefixed("MPRO_"));

        figment.extract().unwrap_or_default()
    }
}

/// Evaluates if a given hour falls inside a configurable acoustic night window.
/// Handles cross-midnight spans (e.g. 23:00 -> 11:00) as well as same-day spans (e.g. 01:00 -> 07:00).
pub fn is_night_window_custom(current_hour: u32, night_start: u32, night_end: u32) -> bool {
    if night_start == night_end {
        return false;
    }
    if night_start < night_end {
        current_hour >= night_start && current_hour < night_end
    } else {
        current_hour >= night_start || current_hour < night_end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cross_midnight_night_window() {
        // 23:00 (11 PM) to 11:00 (11 AM)
        assert!(is_night_window_custom(23, 23, 11));
        assert!(is_night_window_custom(0, 23, 11));
        assert!(is_night_window_custom(3, 23, 11));
        assert!(is_night_window_custom(10, 23, 11));
        assert!(!is_night_window_custom(11, 23, 11));
        assert!(!is_night_window_custom(12, 23, 11));
        assert!(!is_night_window_custom(22, 23, 11));
    }

    #[test]
    fn test_same_day_night_window() {
        // 01:00 to 07:00
        assert!(is_night_window_custom(1, 1, 7));
        assert!(is_night_window_custom(5, 1, 7));
        assert!(!is_night_window_custom(7, 1, 7));
        assert!(!is_night_window_custom(0, 1, 7));
    }

    #[test]
    fn test_figment_default_load() {
        let cfg = ThermalConfig::load(Some("/nonexistent/file.toml"));
        assert_eq!(cfg.night_start_hour, 23);
        assert_eq!(cfg.night_end_hour, 11);
        assert_eq!(cfg.target_setpoint, 54.0);
    }
}
