use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use yeetnyoink::commands;
use yeetnyoink::commands::browser_host::BrowserHostArgs;
#[cfg(target_os = "linux")]
use yeetnyoink::commands::focus_or_cycle::FocusOrCycleArgs;
use yeetnyoink::commands::resize::ResizeMode;
use yeetnyoink::commands::setup::SetupArgs;
use yeetnyoink::config;
use yeetnyoink::engine::topology::Direction;
use yeetnyoink::logging;
use yeetnyoink::profiling::ProfileConfig;

#[derive(Parser)]
#[command(
    name = "yeetnyoink",
    about = "Deep focus/move integration for your configured window manager",
    after_help = "Your config must contain exactly one [wm.<backend>] table, and that table must set `enabled = true`. If you select `wm.macos_native`, that table must also set `floating_focus_strategy` and both Mission Control adjacent-space shortcuts. Current built-in tiling-only (`TilingOnly`) WM backends must not set `floating_focus_strategy`; there is no built-in mixed tiling-and-floating backend yet. Supported strategy names are `radial_center`, `trailing_edge_parallel`, `leading_edge_parallel`, `cross_edge_gap`, `overlap_then_gap`, and `ray_angle`. No runtime window-manager detection or probing occurs."
)]
struct Cli {
    /// Load config from an explicit path; your config must contain exactly one [wm.<backend>] table, that table must set `enabled = true`, `wm.macos_native` requires `floating_focus_strategy`, and current tiling-only backends must leave it unset.
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,

    /// Write debug logs to a file.
    #[arg(long, global = true, value_name = "PATH")]
    log_file: Option<PathBuf>,

    /// Append to --log-file instead of truncating the file.
    #[arg(long, global = true, requires = "log_file")]
    log_append: bool,

    /// Write profiling artifacts to /tmp/yeetnyoink/.
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
    #[cfg(target_os = "linux")]
    FocusOrCycle {
        #[command(flatten)]
        args: FocusOrCycleArgs,
    },
    #[command(hide = true)]
    BrowserHost(BrowserHostArgs),
    /// Install helper integrations like browser native hosts.
    Setup(SetupArgs),
}

impl Cmd {
    fn name(&self) -> &'static str {
        match self {
            Self::Focus { .. } => "focus",
            Self::Move { .. } => "move",
            Self::Resize { .. } => "resize",
            #[cfg(target_os = "linux")]
            Self::FocusOrCycle { .. } => "focus-or-cycle",
            Self::BrowserHost(_) => "browser-host",
            Self::Setup(_) => "setup",
        }
    }
}

fn exit_with_error(err: anyhow::Error) -> ! {
    eprintln!("yeetnyoink: {err:#}");
    std::process::exit(1);
}

fn run_early_command(command: Cmd, config_path: Option<&Path>) -> Result<Option<Cmd>> {
    match command {
        Cmd::BrowserHost(args) => {
            commands::browser_host::run(args, config_path)?;
            Ok(None)
        }
        Cmd::Setup(args) => {
            commands::setup::run(args)?;
            Ok(None)
        }
        command => Ok(Some(command)),
    }
}

fn run_logged_command(command: Cmd) -> Result<()> {
    let command_name = command.name();
    let _span = tracing::info_span!("cli.command", command = command_name).entered();
    match command {
        Cmd::Focus { direction } => commands::focus::run(direction),
        Cmd::Move { direction } => commands::move_win::run(direction),
        Cmd::Resize { direction, mode } => commands::resize::run(direction, mode),
        #[cfg(target_os = "linux")]
        Cmd::FocusOrCycle { args } => commands::focus_or_cycle::run(args),
        Cmd::BrowserHost(_) => unreachable!("browser host mode returns before logging init"),
        Cmd::Setup(_) => unreachable!("setup mode returns before logging init"),
    }
}

fn main() {
    let cli = Cli::parse();
    let Some(command) = run_early_command(cli.command, cli.config.as_deref())
        .unwrap_or_else(|err| exit_with_error(err))
    else {
        return;
    };

    config::prepare_with_path(cli.config.as_deref()).unwrap_or_else(|err| exit_with_error(err));

    let argv = std::env::args().collect::<Vec<_>>();
    let mut logging_session = logging::init(
        cli.log_file.as_deref(),
        cli.log_append,
        cli.profile.then(ProfileConfig::default),
        argv.clone(),
    )
    .unwrap_or_else(|err| exit_with_error(err));
    logging::debug(format!("argv={argv:?}"));
    if let Some(dir) = logging_session.profile_dir() {
        eprintln!("yeetnyoink: profiling -> {}", dir.display());
    }

    if let Err(err) = run_logged_command(command) {
        logging::debug(format!("command failed: {err:#}"));
        if let Err(profile_err) = logging_session.finish() {
            eprintln!("yeetnyoink: profiling finalization failed: {profile_err:#}");
        }
        exit_with_error(err);
    }

    logging::debug("command completed successfully");
    logging_session
        .finish()
        .unwrap_or_else(|err| exit_with_error(err));
}

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::CommandFactory;

    #[test]
    fn cli_help_describes_configured_wm_selection() {
        let help = Cli::command().render_long_help().to_string();

        assert!(help.contains("must contain exactly one [wm.<backend>] table"));
        assert!(help.contains("that table must set `enabled = true`"));
        assert!(help.contains("wm.macos_native"));
        assert!(help.contains("TilingOnly") || help.contains("tiling-only"));
    }
}
