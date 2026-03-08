use clap::{Parser, Subcommand};
use std::path::PathBuf;
use yeet_and_yoink::commands;
use yeet_and_yoink::commands::focus_or_cycle::FocusOrCycleArgs;
use yeet_and_yoink::commands::resize::ResizeMode;
use yeet_and_yoink::config;
use yeet_and_yoink::engine::topology::Direction;
use yeet_and_yoink::logging;
use yeet_and_yoink::profiling::ProfileConfig;

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

fn main() {
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
