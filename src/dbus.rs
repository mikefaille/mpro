use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicI32, Ordering};
use zbus::Connection;

/// Hardware Max RPM Capacities per Physical Fan Channel
pub const FAN_MAX_PCI: i32 = 4500;
pub const FAN_MAX_PS: i32 = 2800;
pub const FAN_MAX_EXHAUST: i32 = 2800;
pub const FAN_MAX_INTAKE: i32 = 2800;
pub const FAN_MAX_BOOSTA: i32 = 5200;
pub const FAN_MAX_BOOSTB: i32 = 5200;

static LAST_SENT_RPM: AtomicI32 = AtomicI32::new(-1);
static LAST_SYSFS_RPM: AtomicI32 = AtomicI32::new(-1);

/// Async D-Bus IPC client connecting directly to system bus daemon endpoints.
pub struct MbpFanDbusClient {
    connection: Option<Connection>,
}

impl MbpFanDbusClient {
    pub async fn new() -> Self {
        let connection = Connection::system().await.ok();
        Self { connection }
    }

    /// Sends RPM override commands over system D-Bus with state deduplication
    pub async fn set_override(&mut self, target_rpm: i32) {
        let last = LAST_SENT_RPM.load(Ordering::Relaxed);
        if target_rpm == last {
            return; // Fast-path deduplication
        }

        if self.connection.is_none() {
            self.connection = Connection::system().await.ok();
        }

        if let Some(ref conn) = self.connection {
            let res = if target_rpm > 800 {
                conn.call_method(
                    Some("org.freedesktop.mbpfan"),
                    "/org/freedesktop/mbpfan",
                    Some("org.freedesktop.mbpfan.Manager"),
                    "SetOverride",
                    &(target_rpm,),
                )
                .await
            } else {
                conn.call_method(
                    Some("org.freedesktop.mbpfan"),
                    "/org/freedesktop/mbpfan",
                    Some("org.freedesktop.mbpfan.Manager"),
                    "ResetOverride",
                    &(),
                )
                .await
            };

            if res.is_ok() {
                LAST_SENT_RPM.store(target_rpm, Ordering::Relaxed);
            }
        }
    }

    #[allow(dead_code)]
    pub async fn reset(&mut self) {
        self.set_override(800).await;
    }
}

/// Reset all 6 physical hardware fan channels to SMC default automatic mode (manual=0, min=800, output=800).
pub fn reset_all_hardware_overrides() {
    let smc_path = Path::new("/sys/devices/platform/applesmc.768");
    if !smc_path.exists() {
        return;
    }

    for idx in 1..=6 {
        let manual_p = smc_path.join(format!("fan{}_manual", idx));
        if manual_p.exists() {
            if let Ok(mut f) = OpenOptions::new().write(true).open(&manual_p) {
                let _ = f.write_all(b"1");
            }
        }
        for suffix in ["_output", "_min"] {
            let p = smc_path.join(format!("fan{}{}", idx, suffix));
            if p.exists() {
                if let Ok(mut f) = OpenOptions::new().write(true).open(p) {
                    let _ = f.write_all(b"800");
                }
            }
        }
        if manual_p.exists() {
            if let Ok(mut f) = OpenOptions::new().write(true).open(&manual_p) {
                let _ = f.write_all(b"0");
            }
        }
    }
    LAST_SYSFS_RPM.store(0, Ordering::Relaxed);
}

/// Direct Linux sysfs hardware driver interface for Apple SMC (`applesmc.768`).
/// Controls all 6 chassis fan channels with atomic state deduplication.
pub fn set_hardware_sysfs_override(target_rpm: i32, enable_manual: bool) {
    let state_key = if enable_manual { target_rpm } else { 0 };
    let last = LAST_SYSFS_RPM.load(Ordering::Relaxed);
    if state_key == last {
        return; // Fast-path: skip duplicate sysfs file open/close cycles
    }

    let smc_path = Path::new("/sys/devices/platform/applesmc.768");
    if !smc_path.exists() {
        return;
    }

    let fan_limits = [
        (1, FAN_MAX_PCI),
        (2, FAN_MAX_PS),
        (3, FAN_MAX_EXHAUST),
        (4, FAN_MAX_INTAKE),
        (5, FAN_MAX_BOOSTA),
        (6, FAN_MAX_BOOSTB),
    ];

    for (idx, max_rpm) in fan_limits {
        let manual_p = smc_path.join(format!("fan{}_manual", idx));

        if enable_manual {
            let capped = target_rpm.min(max_rpm);
            for suffix in ["_output", "_min"] {
                let p = smc_path.join(format!("fan{}{}", idx, suffix));
                if p.exists() {
                    if let Ok(mut f) = OpenOptions::new().write(true).open(p) {
                        let _ = f.write_all(capped.to_string().as_bytes());
                    }
                }
            }
            if manual_p.exists() {
                if let Ok(mut f) = OpenOptions::new().write(true).open(&manual_p) {
                    let _ = f.write_all(b"1");
                }
            }
        } else {
            // Reverting to Apple SMC automatic mode: reset output and min to 800 first!
            if manual_p.exists() {
                if let Ok(mut f) = OpenOptions::new().write(true).open(&manual_p) {
                    let _ = f.write_all(b"1");
                }
            }
            for suffix in ["_output", "_min"] {
                let p = smc_path.join(format!("fan{}{}", idx, suffix));
                if p.exists() {
                    if let Ok(mut f) = OpenOptions::new().write(true).open(p) {
                        let _ = f.write_all(b"800");
                    }
                }
            }
            if manual_p.exists() {
                if let Ok(mut f) = OpenOptions::new().write(true).open(&manual_p) {
                    let _ = f.write_all(b"0");
                }
            }
        }
    }
    LAST_SYSFS_RPM.store(state_key, Ordering::Relaxed);
}
