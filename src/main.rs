use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use yeet_and_yoink::commands;
#[cfg(target_os = "linux")]
use yeet_and_yoink::commands::focus_or_cycle::FocusOrCycleArgs;
use yeet_and_yoink::commands::resize::ResizeMode;
use yeet_and_yoink::config;
use yeet_and_yoink::engine::browser_native::{self, BrowserInstallTarget};
use yeet_and_yoink::engine::kitty_setup::{self, KittyIncludeStatus};
use yeet_and_yoink::engine::zellij_setup;
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
struct BrowserSetupArgs {
    /// Override the yny binary path recorded in the generated wrapper script.
    #[arg(long, value_name = "PATH")]
    yny_path: Option<PathBuf>,
    /// Override the target native-messaging host directory.
    #[arg(long, value_name = "DIR")]
    manifest_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
struct KittySetupArgs {
    /// Override the kitty.conf path that should receive the managed include line.
    #[arg(long, value_name = "PATH")]
    kitty_conf: Option<PathBuf>,
    /// Append the include line without prompting.
    #[arg(long, short = 'y')]
    yes: bool,
}

#[derive(Debug, Clone, Subcommand)]
enum SetupInstaller {
    /// Install the Firefox/LibreWolf native host manifest and wrapper.
    #[command(visible_alias = "librewolf")]
    Firefox(BrowserSetupArgs),
    /// Install the Chromium native host manifest and wrapper.
    Chromium(BrowserSetupArgs),
    /// Install the Google Chrome native host manifest and wrapper.
    #[command(visible_alias = "google-chrome")]
    Chrome(BrowserSetupArgs),
    /// Install the Brave native host manifest and wrapper.
    #[command(visible_alias = "brave-browser")]
    Brave(BrowserSetupArgs),
    /// Install the Microsoft Edge native host manifest and wrapper.
    #[command(visible_alias = "microsoft-edge")]
    Edge(BrowserSetupArgs),
    /// Install kitty remote-control config and offer to wire it into kitty.conf.
    Kitty(KittySetupArgs),
    /// Print zellij plugin install instructions and the hosted release URL.
    Zellij,
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

fn install_kitty_setup(kitty_conf: Option<&std::path::Path>, assume_yes: bool) -> Result<()> {
    let plan = kitty_setup::plan(kitty_conf)?;
    kitty_setup::write_managed_snippet(&plan)?;

    println!("{}", kitty_setup::explanation(&plan));
    println!();

    if kitty_setup::include_present(&plan)? {
        println!("Already configured in {}.", plan.kitty_conf_path.display());
        println!("Restart kitty so the remote-control socket settings take effect.");
        return Ok(());
    }

    if assume_yes {
        return finish_kitty_setup(&plan);
    }

    let stdin = io::stdin();
    let stdout = io::stdout();
    if !stdin.is_terminal() || !stdout.is_terminal() {
        println!("Non-interactive shell detected; leaving kitty.conf unchanged.");
        println!();
        println!("Run this command yourself:");
        println!("{}", plan.manual_command);
        return Ok(());
    }

    loop {
        print!(
            "Press `Y` to append that include line to {} now. Press `N` to print a shell command you can run yourself [Y/n]: ",
            plan.kitty_conf_path.display()
        );
        io::stdout().flush().context("failed to flush setup prompt")?;

        let mut response = String::new();
        stdin
            .read_line(&mut response)
            .context("failed to read setup prompt response")?;
        match response.trim().to_ascii_lowercase().as_str() {
            "" | "y" | "yes" => return finish_kitty_setup(&plan),
            "n" | "no" => {
                println!();
                println!("Run this command yourself:");
                println!("{}", plan.manual_command);
                return Ok(());
            }
            _ => {
                println!("Please answer `Y` or `N`.");
                println!();
            }
        }
    }
}

fn finish_kitty_setup(plan: &kitty_setup::KittySetupPlan) -> Result<()> {
    match kitty_setup::append_include(plan)? {
        KittyIncludeStatus::Added => {
            println!(
                "Added `{}` to {}.",
                plan.include_line,
                plan.kitty_conf_path.display()
            );
        }
        KittyIncludeStatus::AlreadyPresent => {
            println!("Already configured in {}.", plan.kitty_conf_path.display());
        }
    }
    println!("Restart kitty so the remote-control socket settings take effect.");
    Ok(())
}

fn install_zellij_setup() -> Result<()> {
    println!("{}", zellij_setup::instructions());
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
            let result = match installer {
                SetupInstaller::Firefox(args) => install_browser_native_host(
                    BrowserInstallTarget::Firefox,
                    args.yny_path.as_deref(),
                    args.manifest_dir.as_deref(),
                ),
                SetupInstaller::Chromium(args) => install_browser_native_host(
                    BrowserInstallTarget::Chromium,
                    args.yny_path.as_deref(),
                    args.manifest_dir.as_deref(),
                ),
                SetupInstaller::Chrome(args) => install_browser_native_host(
                    BrowserInstallTarget::Chrome,
                    args.yny_path.as_deref(),
                    args.manifest_dir.as_deref(),
                ),
                SetupInstaller::Brave(args) => install_browser_native_host(
                    BrowserInstallTarget::Brave,
                    args.yny_path.as_deref(),
                    args.manifest_dir.as_deref(),
                ),
                SetupInstaller::Edge(args) => install_browser_native_host(
                    BrowserInstallTarget::Edge,
                    args.yny_path.as_deref(),
                    args.manifest_dir.as_deref(),
                ),
                SetupInstaller::Kitty(args) => {
                    install_kitty_setup(args.kitty_conf.as_deref(), args.yes)
                }
                SetupInstaller::Zellij => install_zellij_setup(),
            };
            if let Err(err) = result {
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

        for installer in ["firefox", "chromium", "chrome", "brave", "edge", "kitty", "zellij"] {
            assert!(
                help.contains(installer),
                "setup help should list {installer}: {help}"
            );
        }
        assert!(
            help.contains("Install the Firefox/LibreWolf native host manifest and wrapper"),
            "setup help should describe firefox installer: {help}"
        );
        assert!(
            help.contains("Install kitty remote-control config and offer to wire it into kitty.conf"),
            "setup help should describe kitty installer: {help}"
        );
        assert!(
            help.contains("Print zellij plugin install instructions and the hosted release URL"),
            "setup help should describe zellij installer: {help}"
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

        let kitty = Cli::try_parse_from(["yny", "setup", "kitty"])
            .expect("kitty installer should parse");
        assert!(matches!(
            kitty.command,
            Cmd::Setup {
                installer: SetupInstaller::Kitty(_)
            }
        ));

        let zellij = Cli::try_parse_from(["yny", "setup", "zellij"])
            .expect("zellij installer should parse");
        assert!(matches!(
            zellij.command,
            Cmd::Setup {
                installer: SetupInstaller::Zellij
            }
        ));
    }
}
