use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;
use yeet_and_yoink::commands;
#[cfg(target_os = "linux")]
use yeet_and_yoink::commands::focus_or_cycle::FocusOrCycleArgs;
use yeet_and_yoink::commands::resize::ResizeMode;
use yeet_and_yoink::config;
use yeet_and_yoink::engine::browser_native::{self, BrowserInstallTarget};
use yeet_and_yoink::engine::topology::Direction;
use yeet_and_yoink::logging;
use yeet_and_yoink::profiling::ProfileConfig;

#[derive(Parser)]
#[command(
    name = "yeet-and-yoink",
    about = "Deep focus/move integration for your configured window manager",
    after_help = "Choose the built-in window manager integration in your config via [wm].enabled_integration. No runtime window-manager detection or probing occurs."
)]
struct Cli {
    /// Load config from an explicit path; [wm].enabled_integration selects the built-in WM integration.
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,

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
    #[cfg(target_os = "linux")]
    FocusOrCycle {
        #[command(flatten)]
        args: FocusOrCycleArgs,
    },
    #[command(hide = true)]
    BrowserHost {
        #[arg(value_parser = parse_browser_host_mode)]
        mode: BrowserHostMode,
    },
    /// Install helper integrations like browser native hosts.
    #[command(after_help = "Run `yny setup <installer> --help` for installer-specific options.")]
    Setup {
        #[command(subcommand)]
        installer: SetupInstaller,
    },
}

impl Cmd {
    fn name(&self) -> &'static str {
        match self {
            Self::Focus { .. } => "focus",
            Self::Move { .. } => "move",
            Self::Resize { .. } => "resize",
            #[cfg(target_os = "linux")]
            Self::FocusOrCycle { .. } => "focus-or-cycle",
            Self::BrowserHost { .. } => "browser-host",
            Self::Setup { .. } => "setup",
        }
    }
}

#[derive(Debug, Clone, Args)]
struct SetupArgs {
    /// Override the yny binary path recorded in the generated wrapper script.
    #[arg(long, value_name = "PATH")]
    yny_path: Option<PathBuf>,
    /// Override the target native-messaging host directory.
    #[arg(long, value_name = "DIR")]
    manifest_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Subcommand)]
enum SetupInstaller {
    /// Install the Firefox/LibreWolf native host manifest and wrapper.
    #[command(visible_alias = "librewolf")]
    Firefox(SetupArgs),
    /// Install the Chromium native host manifest and wrapper.
    Chromium(SetupArgs),
    /// Install the Google Chrome native host manifest and wrapper.
    #[command(visible_alias = "google-chrome")]
    Chrome(SetupArgs),
    /// Install the Brave native host manifest and wrapper.
    #[command(visible_alias = "brave-browser")]
    Brave(SetupArgs),
    /// Install the Microsoft Edge native host manifest and wrapper.
    #[command(visible_alias = "microsoft-edge")]
    Edge(SetupArgs),
}

impl SetupInstaller {
    fn target(&self) -> BrowserInstallTarget {
        match self {
            Self::Firefox(_) => BrowserInstallTarget::Firefox,
            Self::Chromium(_) => BrowserInstallTarget::Chromium,
            Self::Chrome(_) => BrowserInstallTarget::Chrome,
            Self::Brave(_) => BrowserInstallTarget::Brave,
            Self::Edge(_) => BrowserInstallTarget::Edge,
        }
    }

    fn args(&self) -> &SetupArgs {
        match self {
            Self::Firefox(args)
            | Self::Chromium(args)
            | Self::Chrome(args)
            | Self::Brave(args)
            | Self::Edge(args) => args,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserHostMode {
    Firefox,
    Chromium,
}

impl BrowserHostMode {
    fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "firefox" | "librewolf" => Ok(Self::Firefox),
            "chromium" | "chrome" | "brave" => Ok(Self::Chromium),
            other => bail!("unsupported browser host mode {other:?}; expected firefox or chromium"),
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

fn parse_browser_host_mode(value: &str) -> std::result::Result<BrowserHostMode, String> {
    BrowserHostMode::parse(value).map_err(|err| err.to_string())
}

fn install_browser_native_host(
    browser: BrowserInstallTarget,
    yny_path: Option<&std::path::Path>,
    manifest_dir: Option<&std::path::Path>,
) -> Result<()> {
    let yny_path = match yny_path {
        Some(path) => path.to_path_buf(),
        None => std::env::current_exe().context("failed to resolve the current yny path")?,
    };
    let report = browser_native::install_native_host(browser, &yny_path, manifest_dir)?;
    println!(
        "Installed {} native host for {}.",
        report.browser.label(),
        report.yny_path.display()
    );
    for path in report.written_paths {
        println!("Wrote {}", path.display());
    }
    println!("{}", report.next_step_hint);
    Ok(())
}

fn main() {
    let cli = Cli::parse();

    match &cli.command {
        Cmd::BrowserHost { mode } => {
            if let Err(err) = config::prepare_with_path(cli.config.as_deref()).and_then(|()| {
                mode.run()
                    .with_context(|| format!("{} browser native host failed", mode.label()))
            }) {
                eprintln!("yeet-and-yoink: {err:#}");
                std::process::exit(1);
            }
            return;
        }
        Cmd::Setup { installer } => {
            let args = installer.args();
            if let Err(err) = install_browser_native_host(
                installer.target(),
                args.yny_path.as_deref(),
                args.manifest_dir.as_deref(),
            ) {
                eprintln!("yeet-and-yoink: {err:#}");
                std::process::exit(1);
            }
            return;
        }
        _ => {}
    }

    if let Err(err) = config::prepare_with_path(cli.config.as_deref()) {
        eprintln!("yeet-and-yoink: {err:#}");
        std::process::exit(1);
    }

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
        match cli.command {
            Cmd::Focus { direction } => commands::focus::run(direction),
            Cmd::Move { direction } => commands::move_win::run(direction),
            Cmd::Resize { direction, mode } => commands::resize::run(direction, mode),
            #[cfg(target_os = "linux")]
            Cmd::FocusOrCycle { args } => commands::focus_or_cycle::run(args),
            Cmd::BrowserHost { .. } => {
                unreachable!("browser host mode returns before logging init")
            }
            Cmd::Setup { .. } => {
                unreachable!("setup mode returns before logging init")
            }
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
    use super::{BrowserHostMode, Cli, Cmd, SetupInstaller};
    use clap::CommandFactory;
    use clap::Parser;

    #[test]
    fn cli_help_describes_configured_wm_selection() {
        let mut command = Cli::command();
        let mut help = Vec::new();
        command
            .write_long_help(&mut help)
            .expect("help text should render");
        let help = String::from_utf8(help).expect("help text should be utf-8");

        assert!(
            help.contains("Choose the built-in window manager integration in your config"),
            "help should explain that WM selection is config driven: {help}"
        );
        assert!(
            help.contains("No runtime window-manager detection or probing occurs"),
            "help should explain that WM probing is disabled: {help}"
        );
    }

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

    #[test]
    fn setup_help_lists_available_installers() {
        let mut command = Cli::command();
        let setup = command
            .find_subcommand_mut("setup")
            .expect("setup subcommand should exist");
        let mut help = Vec::new();
        setup
            .write_long_help(&mut help)
            .expect("setup help should render");
        let help = String::from_utf8(help).expect("help text should be utf-8");

        for installer in ["firefox", "chromium", "chrome", "brave", "edge"] {
            assert!(
                help.contains(installer),
                "setup help should list {installer}: {help}"
            );
        }
        assert!(
            help.contains("Install the Firefox/LibreWolf native host manifest and wrapper"),
            "setup help should describe firefox installer: {help}"
        );
    }

    #[test]
    fn setup_aliases_parse_to_expected_installers() {
        let librewolf = Cli::try_parse_from(["yny", "setup", "librewolf"])
            .expect("librewolf alias should parse");
        assert!(matches!(
            librewolf.command,
            Cmd::Setup {
                installer: SetupInstaller::Firefox(_)
            }
        ));

        let chrome = Cli::try_parse_from(["yny", "setup", "google-chrome"])
            .expect("google-chrome alias should parse");
        assert!(matches!(
            chrome.command,
            Cmd::Setup {
                installer: SetupInstaller::Chrome(_)
            }
        ));
    }
}
