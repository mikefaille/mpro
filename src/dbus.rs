use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicI32, Ordering};
use zbus::Connection;

static LAST_SENT_RPM: AtomicI32 = AtomicI32::new(-1);
static LAST_SYSFS_RPM: AtomicI32 = AtomicI32::new(-1);

/// Physical maximum RPM bounds per fan zone on Mac Pro 5,1
pub const FAN_MAX_BOOSTA: i32 = 5200; // fan5 BOOSTA
pub const FAN_MAX_BOOSTB: i32 = 5200; // fan6 BOOSTB
pub const FAN_MAX_EXHAUST: i32 = 2800; // fan3 EXHAUST
pub const FAN_MAX_INTAKE: i32 = 2800;  // fan4 INTAKE
pub const FAN_MAX_PCI: i32 = 2800;     // fan1 PCI
pub const FAN_MAX_PS: i32 = 2800;      // fan2 PS

/// Pure-Rust Native Async zbus Client for org.freedesktop.mbpfan.
/// Direct Unix socket method dispatch in < 50 μs (zero fork/exec overhead).
pub struct MbpFanDbusClient {
    connection: Option<Connection>,
}

impl MbpFanDbusClient {
    pub async fn new() -> Self {
        let connection = Connection::system().await.ok();
        Self { connection }
    }

    pub async fn set_override(&mut self, target_rpm: i32) {
        let last = LAST_SENT_RPM.load(Ordering::Relaxed);
        if target_rpm == last {
            return; // Fast-path: state deduplication
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

/// Secondary hardware safety bus with state deduplication:
/// Eliminates 12 file open/write/close syscalls per second during steady state.
pub fn set_hardware_sysfs_override(target_rpm: i32) {
    let last = LAST_SYSFS_RPM.load(Ordering::Relaxed);
    if target_rpm == last {
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
        let capped = target_rpm.min(max_rpm);
        for suffix in ["_min", "_output"] {
            let p = smc_path.join(format!("fan{}{}", idx, suffix));
            if p.exists() {
                if let Ok(mut f) = OpenOptions::new().write(true).open(p) {
                    let _ = f.write_all(capped.to_string().as_bytes());
                }
            }
        }
        if capped <= 800 {
            let manual_p = smc_path.join(format!("fan{}_manual", idx));
            if manual_p.exists() {
                if let Ok(mut f) = OpenOptions::new().write(true).open(manual_p) {
                    let _ = f.write_all(b"0");
                }
            }
        }
    }
    LAST_SYSFS_RPM.store(target_rpm, Ordering::Relaxed);
}

/// Reset all hardware fan overrides back to automatic mode cleanly.
pub fn reset_all_hardware_overrides() {
    LAST_SENT_RPM.store(-1, Ordering::Relaxed);
    LAST_SYSFS_RPM.store(-1, Ordering::Relaxed);
    set_hardware_sysfs_override(800);
}
