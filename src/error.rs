use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("usage: {0}")]
    Usage(String),

    #[error("transport error: {0}")]
    Transport(String),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("command error: {0}")]
    Command(String),
}

// ureq رو دستی map می‌کنیم تا enum به کتابخانه HTTP وابسته نباشه
impl From<ureq::Error> for Error {
    fn from(e: ureq::Error) -> Self {
        Error::Transport(e.to_string())
    }
}
