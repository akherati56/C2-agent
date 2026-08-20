use serde::Serialize;

/// دقیقاً همون فیلدهایی که سرور Go در /register انتظار داره
#[derive(Debug, Clone, Serialize)]
pub struct SystemInfo {
    pub host: String,
    pub user: String,
    pub os: String,
}

impl SystemInfo {
    pub fn gather() -> Self {
        Self {
            host: hostname(),
            user: username(),
            os: std::env::consts::OS.to_string(),
        }
    }
}

pub fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown".into())
}

pub fn username() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "unknown".into())
}
