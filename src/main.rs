use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use yeet_and_yoink::commands;
use yeet_and_yoink::commands::focus_or_cycle::FocusOrCycleArgs;
use yeet_and_yoink::commands::resize::ResizeMode;
use yeet_and_yoink::config;
use yeet_and_yoink::engine::topology::Direction;
use yeet_and_yoink::logging;
use yeet_and_yoink::profiling::ProfileConfig;

const BROWSER_HOST_MODE_ENV: &str = "NIRI_DEEP_BROWSER_HOST";

#[derive(Parser)]
#[command(
    name = "yeet-and-yoink",
    about = "Deep focus/move integration for niri"
)]
struct Cli {
    /// Write debug logs to a file.
    #[arg(long, global = true, value_name = "PATH")]
    log_file: Option<PathBuf>,

    /// Append to --log-file instead of truncating the file.
    #[arg(long, global = true, requires = "log_file")]
    log_append: bool,

    /// Write profiling artifacts to /tmp/yeet-and-yoink/.
    #[arg(long, global = true)]
    profile: bool,

    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Focus in a direction, navigating within apps before crossing window boundaries.
    Focus {
        #[arg(value_enum)]
        direction: Direction,
    },
    /// Move in a direction, tearing app buffers into new windows at boundaries.
    Move {
        #[arg(value_enum)]
        direction: Direction,
    },
    /// Resize in a direction, preferring in-app pane resize before compositor fallback.
    Resize {
        #[arg(value_enum)]
        direction: Direction,
        #[arg(value_enum, default_value_t = ResizeMode::Grow)]
        mode: ResizeMode,
    },
    /// Focus existing app instance, cycle through instances, or spawn if absent.
    FocusOrCycle {
        #[command(flatten)]
        args: FocusOrCycleArgs,
    },
}

impl Cmd {
    fn name(&self) -> &'static str {
        match self {
            Self::Focus { .. } => "focus",
            Self::Move { .. } => "move",
            Self::Resize { .. } => "resize",
            Self::FocusOrCycle { .. } => "focus-or-cycle",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserHostMode {
    Firefox,
    Chromium,
}

impl BrowserHostMode {
    fn from_env() -> Result<Option<Self>> {
        match std::env::var(BROWSER_HOST_MODE_ENV) {
            Ok(value) => Ok(Some(Self::parse(&value)?)),
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(std::env::VarError::NotUnicode(_)) => {
                bail!("{BROWSER_HOST_MODE_ENV} must be valid UTF-8")
            }
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "firefox" | "librewolf" => Ok(Self::Firefox),
            "chromium" | "chrome" | "brave" => Ok(Self::Chromium),
            other => bail!(
                "unsupported {BROWSER_HOST_MODE_ENV} value {other:?}; expected firefox or chromium"
            ),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Firefox => "Firefox",
            Self::Chromium => "Chromium",
        }
    }

    fn run(self) -> Result<()> {
        match self {
            Self::Firefox => yeet_and_yoink::adapters::apps::librewolf::run_native_host(),
            Self::Chromium => yeet_and_yoink::adapters::apps::chromium::run_native_host(),
        }
    }
}

fn maybe_run_browser_host() -> Result<bool> {
    let Some(mode) = BrowserHostMode::from_env()? else {
        return Ok(false);
    };
    mode.run()
        .with_context(|| format!("{} browser native host failed", mode.label()))?;
    Ok(true)
}

fn main() {
    match maybe_run_browser_host() {
        Ok(true) => return,
        Ok(false) => {}
        Err(err) => {
            eprintln!("yeet-and-yoink: {err:#}");
            std::process::exit(1);
        }
    }

    let cli = Cli::parse();
    let argv = std::env::args().collect::<Vec<_>>();
    let mut logging_session = match logging::init(
        cli.log_file.as_deref(),
        cli.log_append,
        cli.profile.then(ProfileConfig::default),
        argv.clone(),
    ) {
        Ok(session) => session,
        Err(err) => {
            eprintln!("yeet-and-yoink: {err:#}");
            std::process::exit(1);
        }
    };
    logging::debug(format!("argv={argv:?}"));
    if let Some(dir) = logging_session.profile_dir() {
        eprintln!("yeet-and-yoink: profiling -> {}", dir.display());
    }

    let command_name = cli.command.name();
    let result = {
        let _span = tracing::info_span!("cli.command", command = command_name).entered();
        match {
            let _span = tracing::debug_span!("config.prepare").entered();
            config::prepare()
        } {
            Ok(()) => match cli.command {
                Cmd::Focus { direction } => commands::focus::run(direction),
                Cmd::Move { direction } => commands::move_win::run(direction),
                Cmd::Resize { direction, mode } => commands::resize::run(direction, mode),
                Cmd::FocusOrCycle { args } => commands::focus_or_cycle::run(args),
            },
            Err(err) => Err(err),
        }
    };

    if let Err(e) = result {
        logging::debug(format!("command failed: {e:#}"));
        let profiling_result = logging_session.finish();
        if let Err(profile_err) = profiling_result {
            eprintln!("yeet-and-yoink: profiling finalization failed: {profile_err:#}");
        }
        eprintln!("yeet-and-yoink: {e:#}");
        std::process::exit(1);
    }

    logging::debug("command completed successfully");
    if let Err(profile_err) = logging_session.finish() {
        eprintln!("yeet-and-yoink: {profile_err:#}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::BrowserHostMode;

    #[test]
    fn browser_host_mode_parses_firefox_aliases() {
        assert_eq!(
            BrowserHostMode::parse("firefox").expect("firefox alias should parse"),
            BrowserHostMode::Firefox
        );
        assert_eq!(
            BrowserHostMode::parse("LibreWolf").expect("librewolf alias should parse"),
            BrowserHostMode::Firefox
        );
    }

    #[test]
    fn browser_host_mode_parses_chromium_aliases() {
        assert_eq!(
            BrowserHostMode::parse("chromium").expect("chromium alias should parse"),
            BrowserHostMode::Chromium
        );
        assert_eq!(
            BrowserHostMode::parse("Brave").expect("brave alias should parse"),
            BrowserHostMode::Chromium
        );
    }

    #[test]
    fn browser_host_mode_rejects_unknown_values() {
        assert!(BrowserHostMode::parse("safari").is_err());
    }
}
