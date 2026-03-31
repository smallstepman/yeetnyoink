use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use etcetera::base_strategy::{choose_base_strategy, BaseStrategy};
use std::fs::{self, OpenOptions};
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

use crate::engine::browser_native::{self, BrowserInstallTarget};

const SETUP_AFTER_HELP: &str = "Run `yny setup <installer> --help` for installer-specific options.";
const MANAGED_SNIPPET_NAME: &str = "yeetnyoink.conf";
const MANAGED_SNIPPET: &str = "\
# yeetnyoink kitty integration
allow_remote_control socket-only
listen_on unix:@kitty-{kitty_pid}
";

#[derive(Debug, Clone, Args)]
#[command(after_help = SETUP_AFTER_HELP)]
pub struct SetupArgs {
    #[command(subcommand)]
    installer: SetupInstaller,
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

#[derive(Debug, Clone)]
struct KittySetupPlan {
    kitty_conf_path: PathBuf,
    snippet_path: PathBuf,
    include_line: String,
    manual_command: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KittyIncludeStatus {
    Added,
    AlreadyPresent,
}

pub fn run(args: SetupArgs) -> Result<()> {
    match args.installer {
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
        SetupInstaller::Kitty(args) => install_kitty_setup(args.kitty_conf.as_deref(), args.yes),
        SetupInstaller::Zellij => install_zellij_setup(),
    }
}

pub(crate) fn zellij_release_wasm_url() -> &'static str {
    "https://github.com/smallstepman/yeetnyoink/releases/download/zellij-plugin-latest/yeetnyoink-zellij-break.wasm"
}

fn zellij_release_page_url() -> &'static str {
    "https://github.com/smallstepman/yeetnyoink/releases/tag/zellij-plugin-latest"
}

fn zellij_instructions() -> String {
    format!(
        "Zellij can load the yeetnyoink break plugin straight from GitHub Releases.\n\n\
Add this to your `~/.config/zellij/config.kdl`:\n\n\
load_plugins {{\n    {release_url}\n}}\n\n\
If you already have a `load_plugins` block, add that URL inside it.\n\
Then restart zellij or start a new session.\n\
When zellij prompts for plugin permissions, accept them.\n\n\
`yny` will use the same release URL automatically unless you override \
[runtime.zellij].break_plugin with a local `.wasm` path.\n\n\
Release page:\n  {release_page}",
        release_url = zellij_release_wasm_url(),
        release_page = zellij_release_page_url(),
    )
}

fn install_browser_native_host(
    browser: BrowserInstallTarget,
    yny_path: Option<&Path>,
    manifest_dir: Option<&Path>,
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

fn install_kitty_setup(kitty_conf: Option<&Path>, assume_yes: bool) -> Result<()> {
    let plan = kitty_setup_plan(kitty_conf)?;
    write_managed_snippet(&plan)?;

    println!("{}", kitty_explanation(&plan));
    println!();

    if kitty_include_present(&plan)? {
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
        io::stdout()
            .flush()
            .context("failed to flush setup prompt")?;

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

fn finish_kitty_setup(plan: &KittySetupPlan) -> Result<()> {
    match append_kitty_include(plan)? {
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
    println!("{}", zellij_instructions());
    Ok(())
}

fn kitty_setup_plan(kitty_conf_override: Option<&Path>) -> Result<KittySetupPlan> {
    let kitty_conf_path = resolve_kitty_conf_path(kitty_conf_override)?;
    let snippet_dir = kitty_conf_path
        .parent()
        .context("kitty.conf path has no parent directory")?;
    let snippet_path = snippet_dir.join(MANAGED_SNIPPET_NAME);
    let include_line = format!("include {}", snippet_path.display());
    let manual_command = manual_append_command(&kitty_conf_path, &include_line);
    Ok(KittySetupPlan {
        kitty_conf_path,
        snippet_path,
        include_line,
        manual_command,
    })
}

fn write_managed_snippet(plan: &KittySetupPlan) -> Result<()> {
    if let Some(parent) = plan.snippet_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create kitty snippet directory {}",
                parent.display()
            )
        })?;
    }
    fs::write(&plan.snippet_path, MANAGED_SNIPPET).with_context(|| {
        format!(
            "failed to write kitty setup snippet {}",
            plan.snippet_path.display()
        )
    })?;
    Ok(())
}

fn kitty_include_present(plan: &KittySetupPlan) -> Result<bool> {
    if !plan.kitty_conf_path.exists() {
        return Ok(false);
    }
    let contents = fs::read_to_string(&plan.kitty_conf_path).with_context(|| {
        format!(
            "failed to read kitty config {}",
            plan.kitty_conf_path.display()
        )
    })?;
    Ok(contents.lines().any(|line| line == plan.include_line))
}

fn append_kitty_include(plan: &KittySetupPlan) -> Result<KittyIncludeStatus> {
    if kitty_include_present(plan)? {
        return Ok(KittyIncludeStatus::AlreadyPresent);
    }

    if let Some(parent) = plan.kitty_conf_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create kitty config directory {}",
                parent.display()
            )
        })?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&plan.kitty_conf_path)
        .with_context(|| {
            format!(
                "failed to open kitty config {}",
                plan.kitty_conf_path.display()
            )
        })?;
    write!(file, "\n{}\n", plan.include_line).with_context(|| {
        format!(
            "failed to append kitty include line to {}",
            plan.kitty_conf_path.display()
        )
    })?;
    Ok(KittyIncludeStatus::Added)
}

fn kitty_explanation(plan: &KittySetupPlan) -> String {
    format!(
        "Kitty needs a remote-control socket that detached `yny` invocations can reach.\n\n\
Add this snippet:\n\n\
{snippet}\n\n\
`yny setup kitty` wrote that snippet to:\n  {snippet_path}\n\n\
and kitty still needs this include line in:\n  {kitty_conf_path}\n\n\
{include_line}",
        snippet = indent_block(MANAGED_SNIPPET.trim_end(), "  "),
        snippet_path = plan.snippet_path.display(),
        kitty_conf_path = plan.kitty_conf_path.display(),
        include_line = indent_block(&plan.include_line, "  "),
    )
}

fn resolve_kitty_conf_path(kitty_conf_override: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = kitty_conf_override {
        return resolve_output_path(path, "--kitty-conf");
    }

    let strategy = choose_base_strategy().context("failed to resolve kitty config directory")?;
    Ok(strategy.config_dir().join("kitty").join("kitty.conf"))
}

fn resolve_output_path(path: &Path, flag_name: &str) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()
            .with_context(|| format!("failed to resolve current directory for {flag_name}"))?
            .join(path))
    }
}

fn manual_append_command(kitty_conf_path: &Path, include_line: &str) -> String {
    let parent = kitty_conf_path.parent().unwrap_or_else(|| Path::new("."));
    format!(
        "grep -Fqx {include_line} {kitty_conf} || {{ mkdir -p {kitty_dir} && touch {kitty_conf} && printf '\\n%s\\n' {include_line} >> {kitty_conf}; }}",
        include_line = shell_single_quote_str(include_line),
        kitty_conf = shell_single_quote(kitty_conf_path),
        kitty_dir = shell_single_quote(parent),
    )
}

fn shell_single_quote(path: &Path) -> String {
    shell_single_quote_str(&path.display().to_string())
}

fn shell_single_quote_str(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for ch in value.chars() {
        if ch == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(ch);
        }
    }
    quoted.push('\'');
    quoted
}

fn indent_block(value: &str, prefix: &str) -> String {
    value
        .lines()
        .map(|line| format!("{prefix}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::{
        append_kitty_include, kitty_explanation, kitty_include_present, kitty_setup_plan,
        write_managed_snippet, SetupArgs, SetupInstaller,
    };
    use super::{KittyIncludeStatus, MANAGED_SNIPPET, MANAGED_SNIPPET_NAME};
    use clap::{CommandFactory, Parser, Subcommand};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: TestCmd,
    }

    #[derive(Subcommand)]
    enum TestCmd {
        Setup(SetupArgs),
    }

    const ZELLIJ_RELEASE_TAG: &str = "zellij-plugin-latest";
    const ZELLIJ_RELEASE_ASSET_NAME: &str = "yeetnyoink-zellij-break.wasm";
    const ZELLIJ_RELEASE_REPO: &str = "https://github.com/smallstepman/yeetnyoink";

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "yeetnyoink-kitty-setup-{prefix}-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("temp dir should be created");
        path
    }

    #[test]
    fn instructions_include_load_plugins_snippet_and_release_links() {
        let text = super::zellij_instructions();

        assert!(text.contains("load_plugins {"));
        assert!(text.contains(super::zellij_release_wasm_url()));
        assert!(text.contains(super::zellij_release_page_url()));
        assert!(text.contains("[runtime.zellij].break_plugin"));
        assert!(super::zellij_release_wasm_url().contains(ZELLIJ_RELEASE_TAG));
        assert!(super::zellij_release_wasm_url().contains(ZELLIJ_RELEASE_ASSET_NAME));
        assert!(super::zellij_release_wasm_url().starts_with(ZELLIJ_RELEASE_REPO));
    }

    #[test]
    fn setup_help_lists_available_installers() {
        let mut command = TestCli::command();
        let setup = command
            .find_subcommand_mut("setup")
            .expect("setup subcommand should exist");
        let mut help = Vec::new();
        setup
            .write_long_help(&mut help)
            .expect("setup help should render");
        let help = String::from_utf8(help).expect("help text should be utf-8");

        for installer in [
            "firefox", "chromium", "chrome", "brave", "edge", "kitty", "zellij",
        ] {
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
            help.contains(
                "Install kitty remote-control config and offer to wire it into kitty.conf"
            ),
            "setup help should describe kitty installer: {help}"
        );
        assert!(
            help.contains("Print zellij plugin install instructions and the hosted release URL"),
            "setup help should describe zellij installer: {help}"
        );
    }

    #[test]
    fn setup_aliases_parse_to_expected_installers() {
        let librewolf =
            TestCli::try_parse_from(["yny", "setup", "librewolf"]).expect("alias should parse");
        assert!(matches!(
            librewolf.command,
            TestCmd::Setup(SetupArgs {
                installer: SetupInstaller::Firefox(_)
            })
        ));

        let chrome = TestCli::try_parse_from(["yny", "setup", "google-chrome"])
            .expect("google-chrome alias should parse");
        assert!(matches!(
            chrome.command,
            TestCmd::Setup(SetupArgs {
                installer: SetupInstaller::Chrome(_)
            })
        ));

        let kitty = TestCli::try_parse_from(["yny", "setup", "kitty"])
            .expect("kitty installer should parse");
        assert!(matches!(
            kitty.command,
            TestCmd::Setup(SetupArgs {
                installer: SetupInstaller::Kitty(_)
            })
        ));

        let zellij = TestCli::try_parse_from(["yny", "setup", "zellij"])
            .expect("zellij installer should parse");
        assert!(matches!(
            zellij.command,
            TestCmd::Setup(SetupArgs {
                installer: SetupInstaller::Zellij
            })
        ));
    }

    #[test]
    fn plan_uses_managed_snippet_next_to_kitty_conf() {
        let root = unique_temp_dir("plan");
        let kitty_conf = root.join("kitty").join("kitty.conf");
        let plan = kitty_setup_plan(Some(&kitty_conf)).expect("kitty setup plan should be created");

        assert_eq!(plan.kitty_conf_path, kitty_conf);
        assert_eq!(
            plan.snippet_path,
            root.join("kitty").join(MANAGED_SNIPPET_NAME)
        );
        assert_eq!(
            plan.include_line,
            format!("include {}", plan.snippet_path.display())
        );
    }

    #[test]
    fn write_managed_snippet_writes_expected_content() {
        let root = unique_temp_dir("snippet");
        let kitty_conf = root.join("kitty.conf");
        let plan = kitty_setup_plan(Some(&kitty_conf)).expect("kitty setup plan should be created");

        write_managed_snippet(&plan).expect("managed snippet should be written");

        assert_eq!(
            fs::read_to_string(&plan.snippet_path).expect("snippet should be readable"),
            MANAGED_SNIPPET
        );
    }

    #[test]
    fn append_include_is_idempotent() {
        let root = unique_temp_dir("append");
        let kitty_conf = root.join("kitty.conf");
        let plan = kitty_setup_plan(Some(&kitty_conf)).expect("kitty setup plan should be created");

        assert!(!kitty_include_present(&plan).expect("presence check should work"));
        assert_eq!(
            append_kitty_include(&plan).expect("first append should succeed"),
            KittyIncludeStatus::Added
        );
        assert!(kitty_include_present(&plan).expect("presence check should work"));
        assert_eq!(
            append_kitty_include(&plan).expect("second append should detect existing include"),
            KittyIncludeStatus::AlreadyPresent
        );
    }

    #[test]
    fn explanation_mentions_snippet_and_include_line() {
        let root = unique_temp_dir("explanation");
        let kitty_conf = root.join("kitty.conf");
        let plan = kitty_setup_plan(Some(&kitty_conf)).expect("kitty setup plan should be created");
        let text = kitty_explanation(&plan);

        assert!(text.contains("allow_remote_control socket-only"));
        assert!(text.contains("listen_on unix:@kitty-{kitty_pid}"));
        assert!(text.contains(&plan.include_line));
        assert!(text.contains(&plan.kitty_conf_path.display().to_string()));
    }
}
