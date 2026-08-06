use std::fs;
use std::process::Command;

pub fn get_gchat_webhook_url() -> Option<String> {
    if let Ok(url) = std::env::var("GCHAT_WEBHOOK_URL") {
        if !url.trim().is_empty() {
            return Some(url.trim().to_string());
        }
    }
    if let Ok(content) = fs::read_to_string("/etc/crash_guard_gchat.url") {
        let trimmed = content.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

/// Asynchronous non-blocking Google Chat webhook dispatcher using Tokio + reqwest.
pub async fn send_gchat_notification_async(client: &reqwest::Client, title: &str, message: &str, level: i32) {
    if let Some(webhook_url) = get_gchat_webhook_url() {
        let icon = match level {
            1 => "ℹ️",
            2 => "🚨",
            3 => "🔥",
            _ => "🛡️",
        };

        let text = format!("{} *{}*\n*Host*: `michael-MacPro5-1`\n{}", icon, title, message);
        let payload = serde_json::json!({ "text": text });

        let _ = client
            .post(&webhook_url)
            .header("Content-Type", "application/json; charset=UTF-8")
            .json(&payload)
            .send()
            .await;
    }
}

pub fn send_desktop_notification(title: &str, message: &str) {
    let bus_path = std::path::Path::new("/run/user/1000/bus");
    if !bus_path.exists() {
        return; // Skip desktop notify if user session bus is not active
    }

    let _ = Command::new("runuser")
        .args([
            "-u",
            "michael",
            "--",
            "env",
            "DISPLAY=:0",
            "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus",
            "notify-send",
            "-u",
            "critical",
            title,
            message,
        ])
        .output();
}
