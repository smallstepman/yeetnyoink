use clap::{Parser, Subcommand};
use niri_deep::commands;
use niri_deep::commands::focus_or_cycle::FocusOrCycleArgs;
use niri_deep::commands::resize::ResizeMode;
use niri_deep::config;
use niri_deep::engine::topology::Direction;
use niri_deep::logging;

#[derive(Parser)]
#[command(name = "niri-deep", about = "Deep focus/move integration for niri")]
struct Cli {
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

fn main() {
    logging::init();
    logging::debug(format!("argv={:?}", std::env::args().collect::<Vec<_>>()));

    let cli = Cli::parse();

    let result = match config::prepare() {
        Ok(()) => match cli.command {
            Cmd::Focus { direction } => commands::focus::run(direction),
            Cmd::Move { direction } => commands::move_win::run(direction),
            Cmd::Resize { direction, mode } => commands::resize::run(direction, mode),
            Cmd::FocusOrCycle { args } => commands::focus_or_cycle::run(args),
        },
        Err(err) => Err(err),
    };

    if let Err(e) = result {
        logging::debug(format!("command failed: {e:#}"));
        eprintln!("niri-deep: {e:#}");
        std::process::exit(1);
    }

    logging::debug("command completed successfully");
}
