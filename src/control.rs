use chrono::{Local, Timelike};
use crate::config::{is_night_window_custom, ThermalConfig};
use crate::ml_hazard::HazardResult;
use wide::f64x2;

/// Default Thermal Control Parameters for Intel 5520 IOH Northbridge on Mac Pro 5,1
#[allow(dead_code)]
pub const T_TARGET_SETPOINT: f64 = 54.0; // Default target Northbridge thermal setpoint (°C)
#[allow(dead_code)]
pub const T_CRITICAL_LIMIT: f64 = 63.0;  // Default critical QPI/ESI clock PLL freeze limit (°C)
pub const RPM_MIN_BASELINE: i32 = 800;   // Silent acoustic floor (RPM)
pub const RPM_MAX_CEILING: i32 = 5200;   // Maximum BOOSTA physical fan limit (RPM)

// Physical Control Gains (Proportional, Derivative, Cross-Socket Feedforward)
const K_PROPORTIONAL: f64 = 220.0; // RPM per °C error above setpoint
const K_DERIVATIVE: f64 = 350.0;   // RPM per °C/sec rate of thermal rise
const K_FEEDFORWARD: f64 = 85.0;   // RPM per °C cross-socket CPU heat bleed

/// Output decision structure returned by the cyber-physical control engine.
#[derive(Debug, Clone)]
pub struct ControlDecision {
    /// Alert level: 0 = NOMINAL, 1 = NOTICE, 2 = WARNING, 3 = CRITICAL
    pub alert_level: i32,
    /// Human-readable state indicator string
    pub status_str: &'static str,
    /// Calculated target fan speed in RPM
    pub desired_rpm: i32,
    /// Hardware manual override flag (`true` = manual sysfs control, `false` = Apple SMC automatic curve)
    pub enable_manual: bool,
    /// Proportional feedback term contribution (RPM)
    pub p_term: f64,
    /// Derivative feedback term contribution (RPM)
    pub d_term: f64,
    /// Feedforward heat-bleed contribution (RPM)
    pub ff_term: f64,
}

/// Evaluates Cyber-Physical Energy Balance + EKF Thermal Rate + ML Hazard Forecasting.
///
/// Accepts a custom `ThermalConfig` specifying configurable night window hours and setpoints.
pub fn compute_cyber_physical_control_with_config(
    tn0d_est: f64,
    dt_tn0d_est: f64,
    cpu0_temp: f64,
    hazard: &HazardResult,
    config: &ThermalConfig,
) -> ControlDecision {
    let hour = Local::now().hour();
    let night = is_night_window_custom(hour, config.night_start_hour, config.night_end_hour);
    let effective_rate_threshold = if night { 1.2 } else { 0.6 };
    let effective_hazard_threshold = if night { 0.75 } else { 0.50 };

    let target_setpoint = config.target_setpoint;
    let critical_limit = config.critical_limit;

    // 1. eSIMD Vector Error & Gain packing: vec_error = [error_p, error_d], vec_gains = [K_p, K_d]
    let error_p = (tn0d_est - target_setpoint).max(0.0);
    let error_d = dt_tn0d_est.max(0.0);

    let vec_error = f64x2::from([error_p, error_d]);
    let vec_gains = f64x2::from([K_PROPORTIONAL, K_DERIVATIVE]);

    // Vector multiply computes [p_term, d_term] in 1 SIMD instruction cycle
    let vec_pd_terms = vec_error * vec_gains;
    let pd_arr: [f64; 2] = vec_pd_terms.into();
    let p_term = pd_arr[0];
    let d_term = pd_arr[1];

    // 2. Cross-Socket Fourier Heat Bleed
    let heat_bleed_delta = (cpu0_temp - tn0d_est).max(0.0);
    let ff_term = K_FEEDFORWARD * heat_bleed_delta;

    // 3. Raw Physics Command
    let raw_rpm = RPM_MIN_BASELINE as f64 + p_term + d_term + ff_term;

    // 4. Discrete Cyber-Physical Alert Level Rules & Predictive EKF / ML Hazard Triggers
    let (alert_level, status_str, level_floor) = if tn0d_est >= critical_limit
        || dt_tn0d_est >= 3.0
        || (hazard.p_5m > 0.25 && tn0d_est >= 60.0)
    {
        (3, "CRITICAL", RPM_MAX_CEILING)
    } else if tn0d_est >= 56.5
        || dt_tn0d_est >= 1.5
        || cpu0_temp >= 74.0
        || (hazard.p_15m > 0.05 && tn0d_est >= 55.0)
    {
        (2, "WARNING", 3500)
    } else if tn0d_est >= 53.5
        || dt_tn0d_est >= effective_rate_threshold
        || hazard.p_5m > effective_hazard_threshold
        || hazard.iso_score > 0.70
    {
        (1, "NOTICE", 1400)
    } else {
        (0, "NOMINAL", 800)
    };

    let final_rpm = ((raw_rpm.ceil() as i32).max(level_floor)).min(RPM_MAX_CEILING);

    // Revert to native Apple SMC hardware automatic control (`enable_manual = false`)
    // when in NOMINAL state (alert_level == 0). This allows hardware curves to drop
    // fans to 600-800 RPM for silent night/idle operation.
    let enable_manual = alert_level > 0 || tn0d_est >= 53.5 || cpu0_temp >= 65.0;

    ControlDecision {
        alert_level,
        status_str,
        desired_rpm: final_rpm,
        enable_manual,
        p_term,
        d_term,
        ff_term,
    }
}

/// Backward compatible convenience function using default config parameters.
pub fn compute_cyber_physical_control(
    tn0d_est: f64,
    dt_tn0d_est: f64,
    cpu0_temp: f64,
    hazard: &HazardResult,
) -> ControlDecision {
    compute_cyber_physical_control_with_config(
        tn0d_est,
        dt_tn0d_est,
        cpu0_temp,
        hazard,
        &ThermalConfig::default(),
    )
}
