use std::time::Duration;
use coffee_ldr::loader::Coffee;
use agent::agent::Agent;
use agent::config::Config;
use agent::error::Error;
use agent::executor::{CommandExecutor, OsCommandExecutor};
use agent::transport::{HttpTransport, Transport};
// use coffee_ldr::loader::Coffee;


use tracing_subscriber::EnvFilter;

const USAGE: &str = "usage: agent <server_url> <sleep_seconds> [jitter]\n\
                     example: agent http://192.168.1.10:8080 5 0.2";

fn main() {
    init_logging(); 

    let config = match parse_args() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}\n\n{USAGE}");
            std::process::exit(1);
        }
    };

    let transport = HttpTransport::new(&config);
    let executor = OsCommandExecutor;

    // Dependency injection: پیاده‌سازی‌ها از بیرون تزریق می‌شن
    let agent = Agent::new(config, transport, executor);
    agent.run();
}

fn init_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
}

fn parse_args() -> Result<Config, Error> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.len() < 2 {
        return Err(Error::Usage("not enough arguments".into()));
    }

    let sleep_secs: u64 = args[1]
        .parse()
        .map_err(|_| Error::Usage(format!("invalid sleep seconds: {}", args[1])))?;

    let mut builder = Config::builder()
        .server_url(&args[0])
        .sleep(Duration::from_secs(sleep_secs));

    if let Some(j) = args.get(2) {
        let jitter: f64 = j
            .parse()
            .map_err(|_| Error::Usage(format!("invalid jitter: {j}")))?;
        builder = builder.jitter(jitter);
    }

    builder.build()
}
