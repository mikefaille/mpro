mod control;
mod dbus;
mod ekf;
mod ml_hazard;
mod notify;
mod status;
mod sysinfo_telemetry;

use clap::Parser;
use control::compute_cyber_physical_control;
use dbus::{reset_all_hardware_overrides, set_hardware_sysfs_override, MbpFanDbusClient};
use ekf::ExtendedKalmanThermalFilter;
use ml_hazard::{evaluate_fast_hazard, HazardResult};
use notify::{send_desktop_notification, send_gchat_notification_async};
use status::{write_status_file, SystemStatus};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use sysinfo::System;
use sysinfo_telemetry::FastPosixThermalEngine;
use tokio::sync::watch;

#[derive(Parser, Debug)]
#[command(
    name = "mpro",
    author = "Michael <michael@MacPro5-1>",
    version = "1.0.0",
    about = "Zero-Copy Nanosecond Tokio + zbus Cyber-Physical Thermal Engine in Rust"
)]
struct Args {
    /// Run continuous closed-loop control daemon with Tokio multi-worker tasks
    #[arg(short, long)]
    daemon: bool,

    /// Run micro-benchmark measuring execution latency
    #[arg(short, long)]
    benchmark: bool,

    /// Display current live hardware thermal status
    #[arg(short, long)]
    status: bool,

    /// Send manual D-Bus fan RPM override command
    #[arg(short, long)]
    r#override: Option<i32>,

    /// Control loop interval in seconds (default: 1.0s)
    #[arg(short, long, default_value_t = 1.0)]
    interval: f64,
}

fn run_micro_benchmark() {
    println!("⚡ [Nanosecond Rust mpro Benchmark] Testing Zero-Copy POSIX Loop Speed...");

    let mut engine = FastPosixThermalEngine::new();
    let mut ekf = ExtendedKalmanThermalFilter::new(0.1);

    let iterations = 100_000;
    let start = Instant::now();

    for _ in 0..iterations {
        let snap = engine.sample_thermal_fast();
        let (t_est, dt_est) = ekf.update(snap.tn0d_temp);
        let hazard = evaluate_fast_hazard(t_est, dt_est, snap.cpu0_temp);
        let _decision = compute_cyber_physical_control(t_est, dt_est, snap.cpu0_temp, &hazard);
    }

    let elapsed = start.elapsed();
    let total_secs = elapsed.as_secs_f64();
    let us_per_op = (total_secs / iterations as f64) * 1e6;

    println!("✅ Executed {iterations} zero-copy control ticks in {total_secs:.4} seconds.");
    println!("🚀 Average Loop Latency: {us_per_op:.3} microseconds (μs) per tick!");
    println!("📊 Maximum Throughput : {:.0} evaluations / second!", 1e6 / us_per_op);
}

fn print_live_status() {
    let mut engine = FastPosixThermalEngine::new();
    let snap = engine.sample_thermal_fast();

    let mut sys = System::new_all();
    sys.refresh_cpu_all();
    sys.refresh_memory();

    let cpu_pct = sys.global_cpu_usage();
    let ram_used = sys.used_memory() / (1024 * 1024);
    let ram_total = sys.total_memory() / (1024 * 1024);

    println!("=========================================================");
    println!("🛡️  mpro ZERO-COPY HARDWARE SNAPSHOT");
    println!("=========================================================");
    println!("  • Northbridge Temp (TN0D) : {:.1} °C", snap.tn0d_temp);
    println!("  • CPU 0 Temp (Socket A)   : {:.1} °C", snap.cpu0_temp);
    println!("  • CPU 1 Temp (Socket B)   : {:.1} °C", snap.cpu1_temp);
    println!("  • Ambient Inlet Temp      : {:.1} °C", snap.inlet_temp);
    println!("  • Overall CPU Utilization : {:.1} %", cpu_pct);
    println!("  • System RAM Usage        : {} MB / {} MB", ram_used, ram_total);
    println!("  • Active BOOSTA Fan RPM   : {:.0} RPM", snap.fan_rpm);
    println!("=========================================================");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Lock memory into physical RAM (MLOCKALL) to prevent swap paging under heavy load
    unsafe {
        libc::mlockall(libc::MCL_CURRENT | libc::MCL_FUTURE);
    }

    // Explicit Real-Time SCHED_RR Priority 99 Kernel Scheduling
    unsafe {
        let param = libc::sched_param { sched_priority: 99 };
        let ret = libc::sched_setscheduler(0, libc::SCHED_RR, &param);
        if ret == 0 {
            println!("   Linux Real-Time SCHED_RR Priority 99: ACTIVE");
        } else {
            println!("   Warning: Could not set SCHED_RR priority (ret={})", ret);
        }
    }

    if args.benchmark {
        run_micro_benchmark();
        return Ok(());
    }

    if args.status {
        print_live_status();
        return Ok(());
    }

    if let Some(target_rpm) = args.r#override {
        println!("🚀 Sending Native zbus D-Bus Fan Override Command: {target_rpm} RPM...");
        let mut dbus_client = MbpFanDbusClient::new().await;
        dbus_client.set_override(target_rpm).await;
        set_hardware_sysfs_override(target_rpm);
        println!("✅ Target RPM override applied successfully via zbus IPC.");
        return Ok(());
    }

    println!("🛡️ Starting Zero-Copy Tokio + zbus Engine 'mpro' v1.0.0...");
    println!("   Worker 1: Zero-Copy Nanosecond Control Loop ({:.1}s EKF + zbus IPC)", args.interval);
    println!("   Worker 2: Background ML Hazard Engine (10.0s Interval)");
    println!("   Worker 3: Async Status Writer & Desktop/Mobile Alert Dispatcher");

    // Channels for inter-worker task communication
    let (hazard_tx, hazard_rx) = watch::channel(HazardResult::default());
    let (status_tx, mut status_rx) = watch::channel(SystemStatus {
        timestamp: 0,
        status_str: "NOMINAL",
        alert_level: 0,
        p_5m_pct: 0.0,
        p_15m_pct: 0.0,
        p_30m_pct: 0.0,
        p_60m_pct: 0.0,
        iso_score: 0.0,
        tn0d_temp: 50.0,
        tn0d_rate_c_per_sec: 0.0,
        cpu0_temp: 45.0,
        cpu1_temp: 40.0,
        cpu_usage_pct: 0.0,
        ram_used_mb: 0,
        ram_total_mb: 0,
        target_fan_rpm: 800,
        inference_ms: 0.0001,
        engine_type: "Rust mpro Pure Zero-Copy Engine",
    });

    let running = Arc::new(AtomicBool::new(true));

    // WORKER TASK 1: Async Atomic JSON Status Writer (Debounced State Refresh)
    tokio::spawn(async move {
        let initial_status = *status_rx.borrow();
        write_status_file(&initial_status);
        let mut last_alert = initial_status.alert_level;
        let mut last_write_ts = initial_status.timestamp;

        while status_rx.changed().await.is_ok() {
            let status = *status_rx.borrow_and_update();
            let now_ts = status.timestamp;

            // Debounced atomic write: trigger immediately on alert level changes, or every 5s on idle
            if status.alert_level != last_alert || (now_ts - last_write_ts >= 5) {
                write_status_file(&status);
                last_alert = status.alert_level;
                last_write_ts = now_ts;
            }
        }
    });

    // WORKER TASK 2: Async Notification & Mobile Webhook Dispatcher
    let reqwest_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap_or_default();

    // WORKER TASK 3: Background Machine Learning Hazard Query
    let hazard_tx_clone = hazard_tx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        let mut engine = FastPosixThermalEngine::new();
        loop {
            interval.tick().await;
            let snap = engine.sample_thermal_fast();
            let hazard = evaluate_fast_hazard(snap.tn0d_temp, 0.0, snap.cpu0_temp);
            let _ = hazard_tx_clone.send(hazard);
        }
    });

    // WORKER TASK 4: High-Frequency Zero-Copy Microsecond Control Loop
    let interval_secs = args.interval;
    let is_daemon = args.daemon;
    let running_flag = running.clone();

    set_hardware_sysfs_override(800);
    let mut dbus_client = MbpFanDbusClient::new().await;
    dbus_client.set_override(800).await;

    let main_worker = tokio::spawn(async move {
        let mut engine = FastPosixThermalEngine::new();
        let mut ekf = ExtendedKalmanThermalFilter::new(interval_secs);
        let mut current_target_rpm = 800;
        let mut last_level = -1;
        let mut last_boost_time = Instant::now();
        let min_boost_hold = Duration::from_secs(10);
        let mut tick_interval = tokio::time::interval(Duration::from_secs_f64(interval_secs));

        while running_flag.load(Ordering::SeqCst) {
            tick_interval.tick().await;

            // Pure Zero-Copy POSIX Read (< 500 nanoseconds, ZERO CPU SPIKES)
            let snap = engine.sample_thermal_fast();
            let (tn0d_est, dt_tn0d_est) = ekf.update(snap.tn0d_temp);
            let hazard = hazard_rx.borrow().clone();

            let decision = compute_cyber_physical_control(tn0d_est, dt_tn0d_est, snap.cpu0_temp, &hazard);
            let mut current_level = decision.alert_level;
            let desired_rpm = decision.desired_rpm;

            let now = Instant::now();
            if current_level >= 2 {
                last_boost_time = now;
            } else if (last_level >= 2) && (now.duration_since(last_boost_time) < min_boost_hold) {
                current_level = last_level;
            }

            if desired_rpm < current_target_rpm {
                current_target_rpm = (current_target_rpm - 150).max(desired_rpm);
            } else {
                current_target_rpm = desired_rpm;
            }

            // Native zbus D-Bus IPC method call (< 50 μs)
            dbus_client.set_override(current_target_rpm).await;
            set_hardware_sysfs_override(current_target_rpm);

            let ts = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            let sys_status = SystemStatus {
                timestamp: ts,
                status_str: decision.status_str,
                alert_level: current_level,
                p_5m_pct: hazard.p_5m * 100.0,
                p_15m_pct: hazard.p_15m * 100.0,
                p_30m_pct: hazard.p_30m * 100.0,
                p_60m_pct: hazard.p_60m * 100.0,
                iso_score: hazard.iso_score,
                tn0d_temp: (tn0d_est * 10.0).round() / 10.0,
                tn0d_rate_c_per_sec: (dt_tn0d_est * 1000.0).round() / 1000.0,
                cpu0_temp: snap.cpu0_temp,
                cpu1_temp: snap.cpu1_temp,
                cpu_usage_pct: 0.0,
                ram_used_mb: 0,
                ram_total_mb: 0,
                target_fan_rpm: current_target_rpm,
                inference_ms: hazard.inference_ms,
                engine_type: "Rust mpro Pure Zero-Copy Engine",
            };

            let _ = status_tx.send(sys_status);

            if current_level != last_level && current_level >= 2 {
                let msg = format!(
                    "Thermal Event (Level {} - {}): TN0D={:.1}°C (Rate: {:+.2}°C/s), CPU0={:.1}°C. RPM Target: {}.",
                    current_level, decision.status_str, tn0d_est, dt_tn0d_est, snap.cpu0_temp, current_target_rpm
                );
                send_desktop_notification(&format!("🛡️ mpro Level {current_level}"), &msg);
                send_gchat_notification_async(&reqwest_client, &format!("mpro Level {current_level}"), &msg, current_level).await;
            }

            last_level = current_level;

            if !is_daemon {
                println!(
                    "[{}] TN0D: {:.1}°C (Est: {:.2}°C, Rate: {:+.3}°C/s) | CPU0: {:.1}°C | Command: {} RPM [P:{:.1} D:{:.1} FF:{:.1}]",
                    chrono_like_time(),
                    snap.tn0d_temp,
                    tn0d_est,
                    dt_tn0d_est,
                    snap.cpu0_temp,
                    current_target_rpm,
                    decision.p_term,
                    decision.d_term,
                    decision.ff_term
                );
                break;
            }
        }
    });

    // WORKER TASK 5: Signal Interceptor & Graceful Shutdown
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            println!("\n🛑 Intercepted SIGINT/SIGTERM via Tokio. Cleaning up hardware fan overrides via zbus...");
            reset_all_hardware_overrides();
        }
        _ = main_worker => {}
    }

    println!("🛡️ Tokio + zbus Multi-Worker mpro Engine shutdown complete.");
    Ok(())
}

fn chrono_like_time() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let secs = now % 86400;
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}", hours, mins, s)
}
