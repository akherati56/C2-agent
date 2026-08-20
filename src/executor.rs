use crate::error::Error;

/// قرارداد اجرای دستور؛ پیاده‌سازی‌های مختلف می‌تونن جایگزین بشن
pub trait CommandExecutor: Send + Sync {
    fn execute(&self, command: &str) -> Result<String, Error>;
}

/// اجرای واقعی از طریق shell سیستم‌عامل
pub struct OsCommandExecutor;

impl CommandExecutor for OsCommandExecutor {
    fn execute(&self, command: &str) -> Result<String, Error> {
        #[cfg(windows)]
        let output = std::process::Command::new("cmd").args(["/C", command]).output();

        #[cfg(not(windows))]
        let output = std::process::Command::new("sh").args(["-c", command]).output();

        let output = output.map_err(|e| Error::Command(e.to_string()))?;

        let mut stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr);
        stdout.push_str(&stderr);

        if stdout.trim().is_empty() {
            stdout = "(no output)".to_string();
        }
        Ok(stdout)
    }
}
