use crate::error::Error;

pub trait CommandExecutor: Send + Sync {
    fn execute(&self, command: &str) -> Result<String, Error>;
}

pub struct OsCommandExecutor;

impl CommandExecutor for OsCommandExecutor {
    fn execute(
        &self,
        command: &str,
    ) -> Result<String, Error> {
        #[cfg(windows)]
        let output =
            std::process::Command::new("powershell.exe")
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    command,
                ])
                .output();

        #[cfg(not(windows))]
        let output =
            std::process::Command::new("sh")
                .args(["-c", command])
                .output();

        let output =
            output.map_err(|e| Error::Command(e.to_string()))?;

        let mut stdout =
            String::from_utf8_lossy(
                &output.stdout
            )
                .to_string();

        let stderr =
            String::from_utf8_lossy(
                &output.stderr
            );

        stdout.push_str(&stderr);

        if stdout.trim().is_empty() {
            stdout = "(no output)".to_string();
        }

        Ok(stdout)
    }
}