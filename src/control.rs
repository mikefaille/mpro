use crate::ml_hazard::HazardResult;

pub const T_TARGET_SETPOINT: f64 = 52.0; // Target Northbridge thermal setpoint (°C)
pub const T_CRITICAL_LIMIT: f64 = 63.0;  // Critical QPI/ESI clock PLL freeze limit (°C)
pub const RPM_MIN_BASELINE: i32 = 800;   // Silent desktop fan floor (RPM)
pub const RPM_MAX_CEILING: i32 = 5200;   // Maximum BOOSTA physical limit (RPM)

// Physical Control Gains
const K_PROPORTIONAL: f64 = 220.0; // RPM per °C error
const K_DERIVATIVE: f64 = 350.0;   // RPM per °C/sec rate-of-change
const K_FEEDFORWARD: f64 = 85.0;   // RPM per °C cross-socket heat bleed

pub struct ControlDecision {
    pub alert_level: i32,
    pub status_str: &'static str,
    pub desired_rpm: i32,
    pub p_term: f64,
    pub d_term: f64,
    pub ff_term: f64,
}

/// Evaluates Physics Energy Balance + ML Hazard Forecasting to compute target fan RPM.
pub fn compute_cyber_physical_control(
    tn0d_est: f64,
    dt_tn0d_est: f64,
    cpu0_temp: f64,
    hazard: &HazardResult,
) -> ControlDecision {
    // 1. Proportional Error
    let error = (tn0d_est - T_TARGET_SETPOINT).max(0.0);
    let p_term = K_PROPORTIONAL * error;

    // 2. Derivative Slope Rate-of-Change Acceleration
    let d_term = K_DERIVATIVE * dt_tn0d_est.max(0.0);

    // 3. Cross-Socket Fourier Heat Bleed
    let heat_bleed_delta = (cpu0_temp - tn0d_est).max(0.0);
    let ff_term = K_FEEDFORWARD * heat_bleed_delta;

    // 4. Raw Physics Command
    let raw_rpm = RPM_MIN_BASELINE as f64 + p_term + d_term + ff_term;

    // 5. Discrete Cyber-Physical Alert Level Rules
    let (alert_level, status_str, level_floor) = if tn0d_est >= T_CRITICAL_LIMIT
        || dt_tn0d_est >= 3.0
        || (hazard.p_5m > 0.25 && tn0d_est >= 60.0)
    {
        (3, "CRITICAL", RPM_MAX_CEILING)
    } else if tn0d_est >= 56.0
        || dt_tn0d_est >= 1.5
        || cpu0_temp >= 74.0
        || (hazard.p_15m > 0.05 && tn0d_est >= 54.0)
    {
        (2, "WARNING", 3500)
    } else if tn0d_est >= 53.0 || dt_tn0d_est >= 0.8 {
        (1, "NOTICE", 1800)
    } else {
        (0, "NOMINAL", RPM_MIN_BASELINE)
    };

    let final_rpm = ((raw_rpm.ceil() as i32).max(level_floor)).min(RPM_MAX_CEILING);

    ControlDecision {
        alert_level,
        status_str,
        desired_rpm: final_rpm,
        p_term,
        d_term,
        ff_term,
    }
}
