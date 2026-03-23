use std::collections::BTreeMap;

use anyhow::{anyhow, bail, Result};

use crate::engine::runtime::{self, CommandContext};

const ADAPTER: &str = "mangowc";
const BINARY: &str = "mmsg";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FocusedSnapshot {
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct OutputSnapshot {
    selected: bool,
    focused: FocusedSnapshot,
}

#[derive(Debug, Default)]
pub struct MmsgTransport;

impl MmsgTransport {
    pub fn connect() -> Result<Self> {
        ensure_binary_present()?;
        Ok(Self)
    }

    pub fn dispatch(&self, command: &str, args: &[&str]) -> Result<()> {
        let dispatch = build_dispatch(command, args)?;
        self.run_dispatch(
            &dispatch,
            CommandContext::new(ADAPTER, "mmsg-dispatch")
                .with_target(format!("{command}:{}", args.join(","))),
        )
    }

    pub fn focusdir(&self, direction: &str) -> Result<()> {
        let dispatch = build_focusdir_dispatch(direction)?;
        self.run_dispatch(
            &dispatch,
            CommandContext::new(ADAPTER, "mmsg-dispatch")
                .with_target(format!("focusdir:{direction}")),
        )
    }

    pub fn exchange_client(&self, direction: &str) -> Result<()> {
        let dispatch = build_exchange_client_dispatch(direction)?;
        self.run_dispatch(
            &dispatch,
            CommandContext::new(ADAPTER, "mmsg-dispatch")
                .with_target(format!("exchange_client:{direction}")),
        )
    }

    pub fn tagmon(&self, direction: &str) -> Result<()> {
        let dispatch = build_tagmon_dispatch(direction)?;
        self.run_dispatch(
            &dispatch,
            CommandContext::new(ADAPTER, "mmsg-dispatch")
                .with_target(format!("tagmon:{direction}")),
        )
    }

    pub fn spawn(&self, command: &[String]) -> Result<()> {
        let dispatch = build_spawn_dispatch(command)?;
        self.run_dispatch(
            &dispatch,
            CommandContext::new(ADAPTER, "mmsg-spawn").with_target(command.join(" ")),
        )
    }

    pub fn focused_snapshot(&self) -> Result<FocusedSnapshot> {
        let output = runtime::run_command_output(
            BINARY,
            &["-g"],
            &CommandContext::new(ADAPTER, "mmsg-focused-snapshot"),
        )?;
        if !output.status.success() {
            let stderr = runtime::stderr_text(&output);
            return if stderr.is_empty() {
                Err(anyhow!("{}::mmsg-focused-snapshot failed", ADAPTER))
            } else {
                Err(anyhow!(
                    "{}::mmsg-focused-snapshot failed: {}",
                    ADAPTER,
                    stderr
                ))
            };
        }
        parse_focused_snapshot(&runtime::stdout_text(&output))
    }

    fn run_dispatch(&self, dispatch: &[String], context: CommandContext) -> Result<()> {
        let refs: Vec<&str> = dispatch.iter().map(|s| s.as_str()).collect();
        runtime::run_command_status(BINARY, &refs, &context)
    }
}

fn ensure_binary_present() -> Result<()> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let found = std::env::split_paths(&path).any(|dir| {
        let candidate = dir.join(BINARY);
        candidate
            .metadata()
            .map(|meta| meta.is_file() && is_executable(&meta))
            .unwrap_or(false)
    });

    if found {
        Ok(())
    } else {
        Err(anyhow!("{ADAPTER}: required binary '{BINARY}' not found"))
    }
}

#[cfg(unix)]
fn is_executable(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_meta: &std::fs::Metadata) -> bool {
    false
}

fn parse_u32(value: &str) -> Option<u32> {
    value.parse::<u32>().ok()
}

fn parse_i32(value: &str) -> Option<i32> {
    value.parse::<i32>().ok()
}

fn non_empty(rest: &str) -> Option<String> {
    let trimmed = rest.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn dispatch_args(command: &str, args: &[&str]) -> Vec<String> {
    let mut parts = Vec::with_capacity(args.len() + 1);
    parts.push(command.to_string());
    parts.extend(args.iter().map(|s| s.to_string()));
    vec!["-d".to_string(), parts.join(",")]
}

fn build_dispatch(command: &str, args: &[&str]) -> Result<Vec<String>> {
    if args.len() > 5 {
        bail!(
            "mmsg dispatch supports at most 5 args, got {} for command {:?}",
            args.len(),
            command
        );
    }

    for arg in args {
        if arg.contains(',') {
            bail!("mmsg dispatch arguments cannot contain commas: {:?}", arg);
        }
        if arg.trim() != *arg {
            bail!(
                "mmsg dispatch arguments cannot have leading or trailing whitespace: {:?}",
                arg
            );
        }
    }

    Ok(dispatch_args(command, args))
}

pub fn build_spawn_dispatch(command: &[String]) -> Result<Vec<String>> {
    if command.is_empty() {
        bail!("mmsg spawn command cannot be empty");
    }
    if let Some(arg) = command.iter().find(|arg| arg.is_empty()) {
        bail!("mmsg spawn command arguments cannot be empty: {:?}", arg);
    }
    if let Some(arg) = command.iter().find(|arg| arg.contains(',')) {
        bail!(
            "mmsg spawn command arguments cannot contain commas in mmsg spawn dispatch: {:?}",
            arg
        );
    }
    if let Some(arg) = command.iter().find(|arg| arg.chars().any(char::is_whitespace)) {
        bail!(
            "mmsg spawn command arguments cannot contain whitespace because mangowc spawn expects a single space-delimited command string: {:?}",
            arg
        );
    }

    let joined = command.join(" ");
    build_dispatch("spawn", &[joined.as_str()])
}

pub fn build_focusdir_dispatch(direction: &str) -> Result<Vec<String>> {
    build_dispatch("focusdir", &[direction])
}

pub fn build_exchange_client_dispatch(direction: &str) -> Result<Vec<String>> {
    build_dispatch("exchange_client", &[direction])
}

pub fn build_tagmon_dispatch(direction: &str) -> Result<Vec<String>> {
    build_dispatch("tagmon", &[direction])
}

pub fn parse_focused_snapshot(input: &str) -> Result<FocusedSnapshot> {
    let mut outputs = BTreeMap::<String, OutputSnapshot>::new();
    for line in input.lines() {
        let mut parts = line.split_whitespace();
        let output = match parts.next() {
            Some(v) => v,
            None => continue,
        };
        let key = match parts.next() {
            Some(v) => v,
            None => continue,
        };
        let rest = parts.collect::<Vec<_>>().join(" ");
        let entry = outputs.entry(output.to_string()).or_default();
        match key {
            "selmon" => entry.selected = rest.trim() == "1",
            "appid" => entry.focused.app_id = non_empty(&rest),
            "title" => entry.focused.title = non_empty(&rest),
            "x" => entry.focused.x = parse_i32(&rest),
            "y" => entry.focused.y = parse_i32(&rest),
            "width" => entry.focused.width = parse_u32(&rest),
            "height" => entry.focused.height = parse_u32(&rest),
            _ => {}
        }
    }

    if outputs.is_empty() {
        return Ok(FocusedSnapshot::default());
    }

    let mut selected = outputs
        .values()
        .filter(|snapshot| snapshot.selected)
        .map(|snapshot| snapshot.focused.clone());

    match (selected.next(), selected.next()) {
        (Some(snapshot), None) => Ok(snapshot),
        (Some(_), Some(_)) => bail!("mangowc: multiple selected mmsg outputs in focused snapshot"),
        (None, _) if outputs.len() == 1 => Ok(outputs
            .into_values()
            .next()
            .expect("single output snapshot must exist")
            .focused),
        (None, _) => bail!("mangowc: unable to determine selected mmsg output from focused snapshot"),
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::sync::OnceLock;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn path_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct PathGuard {
        previous: Option<OsString>,
        fake_dirs: Vec<PathBuf>,
    }

    impl PathGuard {
        fn install(fake_dirs: Vec<PathBuf>) -> Result<Self> {
            for dir in &fake_dirs {
                if dir.exists() {
                    fs::remove_dir_all(dir)?;
                }
                fs::create_dir_all(dir)?;
            }
            let previous = std::env::var_os("PATH");
            let joined = std::env::join_paths(&fake_dirs)?;
            std::env::set_var("PATH", joined);
            Ok(Self {
                previous,
                fake_dirs,
            })
        }
    }

    impl Drop for PathGuard {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                std::env::set_var("PATH", previous);
            } else {
                std::env::remove_var("PATH");
            }
            for dir in &self.fake_dirs {
                let _ = fs::remove_dir_all(dir);
            }
        }
    }

    fn run_with_fake_path_that_hides_mmsg() -> Result<MmsgTransport> {
        let _guard = path_lock().lock().expect("path lock poisoned");
        let fake_dir =
            std::env::temp_dir().join(format!("yeetnyoink-mmsg-missing-{}", std::process::id()));
        let _path = PathGuard::install(vec![fake_dir])?;
        MmsgTransport::connect()
    }

    #[cfg(unix)]
    fn run_with_fake_path_that_only_has_mmsg() -> Result<MmsgTransport> {
        let _guard = path_lock().lock().expect("path lock poisoned");
        let base =
            std::env::temp_dir().join(format!("yeetnyoink-mmsg-connect-{}", std::process::id()));
        let bin_dir = base.join("bin");
        let mmsg = bin_dir.join(BINARY);
        let _path = PathGuard::install(vec![bin_dir])?;
        fs::write(&mmsg, "#!/bin/sh\nexit 0\n")?;
        let mut perms = fs::metadata(&mmsg)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&mmsg, perms)?;
        MmsgTransport::connect()
    }

    #[test]
    fn mangowc_mmsg_dispatch_args_encode_directional_command() {
        assert_eq!(
            dispatch_args("focusdir", &["left"]),
            vec!["-d".to_string(), "focusdir,left".to_string()]
        );
    }

    #[test]
    fn mangowc_mmsg_builders_encode_directional_commands() {
        assert_eq!(
            build_focusdir_dispatch("left").unwrap(),
            vec!["-d".to_string(), "focusdir,left".to_string()]
        );
        assert_eq!(
            build_exchange_client_dispatch("right").unwrap(),
            vec!["-d".to_string(), "exchange_client,right".to_string()]
        );
        assert_eq!(
            build_tagmon_dispatch("next").unwrap(),
            vec!["-d".to_string(), "tagmon,next".to_string()]
        );
    }

    #[test]
    fn mangowc_mmsg_directional_builders_reject_invalid_dispatch_args() {
        assert!(build_focusdir_dispatch(" left").is_err());
        assert!(build_exchange_client_dispatch("right ").is_err());
        assert!(build_tagmon_dispatch("left,right").is_err());
    }

    #[test]
    fn mangowc_mmsg_dispatch_builder_rejects_more_than_five_args() {
        let err = build_dispatch("setoption", &["1", "2", "3", "4", "5", "6"]).unwrap_err();
        assert!(err.to_string().contains("at most 5 args"));
    }

    #[test]
    fn mangowc_mmsg_spawn_rejects_empty_command_vector() {
        assert!(build_spawn_dispatch(&[]).is_err());
    }

    #[test]
    fn mangowc_mmsg_spawn_rejects_args_containing_commas() {
        let err = build_spawn_dispatch(&[
            "foot".to_string(),
            "--title".to_string(),
            "left,right".to_string(),
        ])
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("cannot contain commas in mmsg spawn dispatch")
        );
    }

    #[test]
    fn mangowc_mmsg_spawn_joins_command_vector_into_single_dispatch_arg() {
        assert_eq!(
            build_spawn_dispatch(&[
                "foot".to_string(),
                "--app-id".to_string(),
                "smoke".to_string(),
            ])
            .unwrap(),
            vec![
                "-d".to_string(),
                "spawn,foot --app-id smoke".to_string(),
            ]
        );
    }

    #[test]
    fn mangowc_mmsg_spawn_rejects_whitespace_containing_args() {
        let err = build_spawn_dispatch(&[
            "foot".to_string(),
            "--title".to_string(),
            "my terminal title".to_string(),
        ])
        .unwrap_err();
        assert!(err.to_string().contains("cannot contain whitespace"));
    }

    #[test]
    fn mangowc_mmsg_parse_focused_snapshot_reads_appid_title_and_geometry() {
        let sample = "\
Virtual-1 title zsh in repo\n\
Virtual-1 appid foot\n\
Virtual-1 x 1820\n\
Virtual-1 y 16\n\
Virtual-1 width 1764\n\
Virtual-1 height 1110\n";
        let snap = parse_focused_snapshot(sample).unwrap();
        assert_eq!(snap.app_id.as_deref(), Some("foot"));
        assert_eq!(snap.title.as_deref(), Some("zsh in repo"));
        assert_eq!(snap.x, Some(1820));
        assert_eq!(snap.y, Some(16));
        assert_eq!(snap.width, Some(1764));
        assert_eq!(snap.height, Some(1110));
    }

    #[test]
    fn mangowc_mmsg_parse_focused_snapshot_selects_active_output() {
        let sample = "\
HDMI-A-1 selmon 0\n\
HDMI-A-1 title secondary shell\n\
HDMI-A-1 appid foot\n\
HDMI-A-1 x 20\n\
HDMI-A-1 y 30\n\
HDMI-A-1 width 800\n\
HDMI-A-1 height 600\n\
Virtual-1 selmon 1\n\
Virtual-1 title focused shell\n\
Virtual-1 appid kitty\n\
Virtual-1 x 1820\n\
Virtual-1 y 16\n\
Virtual-1 width 1764\n\
Virtual-1 height 1110\n";
        let snap = parse_focused_snapshot(sample).unwrap();
        assert_eq!(snap.app_id.as_deref(), Some("kitty"));
        assert_eq!(snap.title.as_deref(), Some("focused shell"));
        assert_eq!(snap.x, Some(1820));
        assert_eq!(snap.y, Some(16));
        assert_eq!(snap.width, Some(1764));
        assert_eq!(snap.height, Some(1110));
    }

    #[test]
    fn mangowc_mmsg_parse_focused_snapshot_rejects_ambiguous_multi_output_payload() {
        let sample = "\
HDMI-A-1 title secondary shell\n\
HDMI-A-1 appid foot\n\
Virtual-1 title focused shell\n\
Virtual-1 appid kitty\n";
        let err = parse_focused_snapshot(sample).unwrap_err();
        assert!(err.to_string().contains("selected mmsg output"));
    }

    #[test]
    fn mangowc_mmsg_reports_missing_binary_cleanly() {
        let err = run_with_fake_path_that_hides_mmsg().unwrap_err();
        assert!(err.to_string().contains("mmsg"));
    }

    #[cfg(unix)]
    #[test]
    fn mangowc_mmsg_connect_succeeds_with_path_mmsg_without_which() {
        run_with_fake_path_that_only_has_mmsg().expect("connect should find PATH mmsg directly");
    }
}
