use std::time::Duration;

use crate::error::Error;

const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36";

#[derive(Debug, Clone)]
pub struct Config {
    pub server_url: String,
    pub sleep: Duration,
    pub jitter: f64, // مثال: 0.2 یعنی ±۲۰٪
    pub retry_delay: Duration,
    pub user_agent: String,
}

impl Config {
    pub fn builder() -> ConfigBuilder {
        ConfigBuilder::default()
    }
}
 
pub struct ConfigBuilder {
    server_url: Option<String>,
    sleep: Duration,
    jitter: f64,
    retry_delay: Duration,
    user_agent: Option<String>,
}

impl ConfigBuilder {
    pub fn server_url(mut self, url: impl Into<String>) -> Self {
        self.server_url = Some(url.into());
        self
    }

    pub fn sleep(mut self, d: Duration) -> Self {
        self.sleep = d;
        self
    }

    pub fn jitter(mut self, j: f64) -> Self {
        self.jitter = j;
        self
    }

    pub fn retry_delay(mut self, d: Duration) -> Self {
        self.retry_delay = d;
        self
    }

    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = Some(ua.into());
        self
    }

    pub fn build(self) -> Result<Config, Error> {
        let server_url = self
            .server_url
            .ok_or_else(|| Error::Usage("server_url is required".into()))?;

        if !(server_url.starts_with("http://") || server_url.starts_with("https://")) {
            return Err(Error::Usage(
                "server_url must start with http:// or https://".into(),
            ));
        }
        if self.sleep.is_zero() {
            return Err(Error::Usage("sleep must be greater than zero".into()));
        }
        if !(0.0..=0.5).contains(&self.jitter) {
            return Err(Error::Usage("jitter must be between 0.0 and 0.5".into()));
        }

        Ok(Config {
            server_url: server_url.trim_end_matches('/').to_string(),
            sleep: self.sleep,
            jitter: self.jitter,
            retry_delay: self.retry_delay,
            user_agent: self
                .user_agent
                .unwrap_or_else(|| DEFAULT_USER_AGENT.to_string()),
        })
    }
}

impl Default for ConfigBuilder {
    fn default() -> Self {
        Self {
            server_url: None,
            sleep: Duration::from_secs(5),
            jitter: 0.2,
            retry_delay: Duration::from_secs(10),
            user_agent: None,
        }
    }
}
