use serde::Serialize;
use std::fs::File;

#[derive(Serialize, Clone, Copy)]
pub struct SystemStatus {
    pub timestamp: u64,
    pub status_str: &'static str,
    pub alert_level: i32,
    pub p_5m_pct: f64,
    pub p_15m_pct: f64,
    pub p_30m_pct: f64,
    pub p_60m_pct: f64,
    pub iso_score: f64,
    pub tn0d_temp: f64,
    pub tn0d_rate_c_per_sec: f64,
    pub cpu0_temp: f64,
    pub cpu1_temp: f64,
    pub cpu_usage_pct: f32,
    pub ram_used_mb: u64,
    pub ram_total_mb: u64,
    pub target_fan_rpm: i32,
    pub inference_ms: f64,
    pub engine_type: &'static str,
}

pub fn write_status_file(status: &SystemStatus) {
    let tmp_path = "/tmp/crash_guard.json.tmp";
    let target_path = "/tmp/crash_guard.json";

    if let Ok(mut f) = File::create(tmp_path) {
        if serde_json::to_writer_pretty(&mut f, status).is_ok() {
            let _ = std::fs::rename(tmp_path, target_path);
        }
    }
}
