use std::sync::OnceLock;

use tracing_subscriber::EnvFilter;

fn debug_env_enabled() -> bool {
    let value = match std::env::var("NIRI_DEEP_DEBUG") {
        Ok(value) => value,
        Err(_) => return false,
    };

    let value = value.trim().to_ascii_lowercase();
    !(value.is_empty() || value == "0" || value == "false" || value == "off" || value == "no")
}

pub fn init() {
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        let default_filter = if debug_env_enabled() { "debug" } else { "off" };
        let env_filter =
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter));

        if let Err(err) = tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_target(false)
            .without_time()
            .compact()
            .try_init()
        {
            eprintln!("niri-deep: failed to initialize logging: {err}");
        }
    });
}

pub fn debug(message: impl std::fmt::Display) {
    tracing::debug!("{message}");
}
