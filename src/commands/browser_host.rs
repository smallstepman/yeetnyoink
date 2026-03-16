use anyhow::{bail, Context, Result};
use clap::Args;
use std::path::Path;

use crate::config;

#[derive(Debug, Clone, Args)]
pub struct BrowserHostArgs {
    #[arg(value_parser = parse_browser_host_mode)]
    mode: BrowserHostMode,
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
            Self::Firefox => crate::adapters::apps::librewolf::run_native_host(),
            Self::Chromium => crate::adapters::apps::chromium::run_native_host(),
        }
    }
}

fn parse_browser_host_mode(value: &str) -> std::result::Result<BrowserHostMode, String> {
    BrowserHostMode::parse(value).map_err(|err| err.to_string())
}

pub fn run(args: BrowserHostArgs, config_path: Option<&Path>) -> Result<()> {
    config::prepare_with_path(config_path).and_then(|()| {
        args.mode
            .run()
            .with_context(|| format!("{} browser native host failed", args.mode.label()))
    })
}

#[cfg(test)]
mod tests {
    use super::{BrowserHostArgs, BrowserHostMode};
    use clap::{Parser, Subcommand};

    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: TestCmd,
    }

    #[derive(Subcommand)]
    enum TestCmd {
        BrowserHost(BrowserHostArgs),
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
    fn browser_host_args_parse_mode_aliases() {
        let firefox =
            TestCli::try_parse_from(["yny", "browser-host", "librewolf"]).expect("alias parses");
        assert!(matches!(firefox.command, TestCmd::BrowserHost(_)));
    }
}
