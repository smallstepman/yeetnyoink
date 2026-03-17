use std::fs::{File, OpenOptions};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use anyhow::{Context, Result};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter, Layer};

use crate::config;
use crate::profiling::{ProfileConfig, ProfilingSession};

#[derive(Default)]
pub struct LoggingSession {
    profiling: Option<ProfilingSession>,
}

impl LoggingSession {
    pub fn profile_dir(&self) -> Option<&Path> {
        self.profiling.as_ref().map(ProfilingSession::artifact_dir)
    }

    pub fn finish(&mut self) -> Result<()> {
        if let Some(profiling) = self.profiling.as_mut() {
            profiling.finish()?;
        }
        Ok(())
    }
}

fn debug_enabled() -> bool {
    let level = config::logging_level();
    matches!(level, config::LogLevel::Debug | config::LogLevel::Trace)
}

fn open_log_file(path: &Path, append: bool) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).write(true);
    if append {
        options.append(true);
    } else {
        options.truncate(true);
    }
    options.open(path)
}

fn default_fmt_filter(log_file: Option<&Path>) -> EnvFilter {
    let default_filter = if debug_enabled() || log_file.is_some() {
        "debug"
    } else {
        "off"
    };
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter))
}

fn maybe_open_log_file(log_file: Option<&Path>, append: bool) -> Option<File> {
    log_file.and_then(|path| match open_log_file(path, append) {
        Ok(file) => Some(file),
        Err(err) => {
            eprintln!(
                "yeetnyoink: failed to open log file {}: {err}",
                path.display()
            );
            None
        }
    })
}

pub fn init(
    log_file: Option<&Path>,
    append: bool,
    profile: Option<ProfileConfig>,
    argv: Vec<String>,
) -> Result<LoggingSession> {
    static INIT: OnceLock<()> = OnceLock::new();
    if INIT.get().is_some() {
        return Ok(LoggingSession::default());
    }

    let mut session = LoggingSession::default();
    let log_file_handle = maybe_open_log_file(log_file, append);

    match profile {
        Some(profile) => {
            let mut profiling = ProfilingSession::start(&profile, argv)
                .context("failed to initialize profiling")?;

            match log_file_handle {
                Some(file) => {
                    let chrome = profiling.chrome_layer();
                    let flame = profiling.flame_layer()?;
                    let summary = profiling.summary_layer();
                    tracing_subscriber::registry()
                        .with(chrome)
                        .with(flame)
                        .with(summary)
                        .with(
                            fmt::layer()
                                .with_target(false)
                                .without_time()
                                .compact()
                                .with_writer(Mutex::new(file))
                                .with_filter(default_fmt_filter(log_file)),
                        )
                        .try_init()
                        .context("failed to initialize tracing subscriber")?;
                }
                None => {
                    let chrome = profiling.chrome_layer();
                    let flame = profiling.flame_layer()?;
                    let summary = profiling.summary_layer();
                    tracing_subscriber::registry()
                        .with(chrome)
                        .with(flame)
                        .with(summary)
                        .with(
                            fmt::layer()
                                .with_target(false)
                                .without_time()
                                .compact()
                                .with_filter(default_fmt_filter(log_file)),
                        )
                        .try_init()
                        .context("failed to initialize tracing subscriber")?;
                }
            }

            session.profiling = Some(profiling);
        }
        None => match log_file_handle {
            Some(file) => tracing_subscriber::registry()
                .with(
                    fmt::layer()
                        .with_target(false)
                        .without_time()
                        .compact()
                        .with_writer(Mutex::new(file))
                        .with_filter(default_fmt_filter(log_file)),
                )
                .try_init()
                .context("failed to initialize tracing subscriber")?,
            None => tracing_subscriber::registry()
                .with(
                    fmt::layer()
                        .with_target(false)
                        .without_time()
                        .compact()
                        .with_filter(default_fmt_filter(log_file)),
                )
                .try_init()
                .context("failed to initialize tracing subscriber")?,
        },
    }

    let _ = INIT.set(());
    Ok(session)
}

pub fn debug(message: impl std::fmt::Display) {
    tracing::debug!("{message}");
}
