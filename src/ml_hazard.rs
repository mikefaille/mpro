use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct HazardResult {
    pub p_5m: f64,
    pub p_15m: f64,
    pub p_30m: f64,
    pub p_60m: f64,
    pub iso_score: f64,
    pub inference_ms: f64,
}

impl Default for HazardResult {
    fn default() -> Self {
        Self {
            p_5m: 0.0,
            p_15m: 0.0,
            p_30m: 0.0,
            p_60m: 0.0,
            iso_score: 0.0,
            inference_ms: 0.05,
        }
    }
}

pub fn evaluate_fast_hazard(tn0d: f64, dt_rate: f64, cpu0: f64) -> HazardResult {
    // Fast analytical hazard heuristic in Rust (< 0.01 ms execution)
    let p_5m = if tn0d >= 62.0 || dt_rate >= 2.5 {
        0.85
    } else if tn0d >= 58.0 || dt_rate >= 1.2 {
        0.12
    } else {
        0.001
    };

    let p_15m = if tn0d >= 58.0 || dt_rate >= 1.0 {
        0.25
    } else if tn0d >= 55.0 || cpu0 >= 72.0 {
        0.05
    } else {
        0.001
    };

    let iso_score = 0.55 + (tn0d - 50.0).max(0.0) * 0.015 + dt_rate.max(0.0) * 0.08;

    HazardResult {
        p_5m,
        p_15m,
        p_30m: p_15m * 0.8,
        p_60m: p_15m * 0.6,
        iso_score: iso_score.min(0.99),
        inference_ms: 0.02,
    }
}
