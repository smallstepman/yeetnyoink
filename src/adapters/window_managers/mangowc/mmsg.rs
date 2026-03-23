use anyhow::{anyhow, bail, Context, Result};

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

#[derive(Debug, Default)]
pub struct MmsgTransport;

impl MmsgTransport {
    pub fn connect() -> Result<Self> {
        ensure_binary_present()?;
        Ok(Self)
    }

    pub fn dispatch(&self, command: &str, args: &[&str]) -> Result<()> {
        let dispatch = dispatch_args(command, args);
        self.run_dispatch(
            &dispatch,
            CommandContext::new(ADAPTER, "mmsg-dispatch")
                .with_target(format!("{command}:{}", args.join(","))),
        )
    }

    pub fn focusdir(&self, direction: &str) -> Result<()> {
        let dispatch = build_focusdir_dispatch(direction);
        self.run_dispatch(
            &dispatch,
            CommandContext::new(ADAPTER, "mmsg-dispatch")
                .with_target(format!("focusdir:{direction}")),
        )
    }

    pub fn exchange_client(&self, direction: &str) -> Result<()> {
        let dispatch = build_exchange_client_dispatch(direction);
        self.run_dispatch(
            &dispatch,
            CommandContext::new(ADAPTER, "mmsg-dispatch")
                .with_target(format!("exchange_client:{direction}")),
        )
    }

    pub fn tagmon(&self, direction: &str) -> Result<()> {
        let dispatch = build_tagmon_dispatch(direction);
        self.run_dispatch(
            &dispatch,
            CommandContext::new(ADAPTER, "mmsg-dispatch")
                .with_target(format!("tagmon:{direction}")),
        )
    }

    pub fn spawn(&self, command: &[String]) -> Result<()> {
        let dispatch = build_spawn_dispatch(command)?;
        self.run_dispatch(&dispatch, CommandContext::new(ADAPTER, "mmsg-spawn"))
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
    runtime::run_command_status(
        "which",
        &[BINARY],
        &CommandContext::new(ADAPTER, "mmsg-connect"),
    )
    .with_context(|| format!("{ADAPTER}: required binary '{BINARY}' not found"))
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

pub fn dispatch_args(command: &str, args: &[&str]) -> Vec<String> {
    let mut parts = Vec::with_capacity(args.len() + 1);
    parts.push(command.to_string());
    parts.extend(args.iter().map(|s| s.to_string()));
    vec!["-d".to_string(), parts.join(",")]
}

pub fn build_spawn_dispatch(command: &[String]) -> Result<Vec<String>> {
    if command.is_empty() {
        bail!("mmsg spawn command cannot be empty");
    }
    if let Some(arg) = command.iter().find(|arg| arg.contains(',')) {
        bail!(
            "mmsg spawn command arguments cannot contain commas in mmsg spawn dispatch: {:?}",
            arg
        );
    }
    let refs: Vec<&str> = command.iter().map(|s| s.as_str()).collect();
    Ok(dispatch_args("spawn", &refs))
}

pub fn build_focusdir_dispatch(direction: &str) -> Vec<String> {
    dispatch_args("focusdir", &[direction])
}

pub fn build_exchange_client_dispatch(direction: &str) -> Vec<String> {
    dispatch_args("exchange_client", &[direction])
}

pub fn build_tagmon_dispatch(direction: &str) -> Vec<String> {
    dispatch_args("tagmon", &[direction])
}

pub fn parse_focused_snapshot(input: &str) -> Result<FocusedSnapshot> {
    let mut snap = FocusedSnapshot::default();
    for line in input.lines() {
        let mut parts = line.split_whitespace();
        let _output = match parts.next() {
            Some(v) => v,
            None => continue,
        };
        let key = match parts.next() {
            Some(v) => v,
            None => continue,
        };
        let rest = parts.collect::<Vec<_>>().join(" ");
        match key {
            "appid" => snap.app_id = non_empty(&rest),
            "title" => snap.title = non_empty(&rest),
            "x" => snap.x = parse_i32(&rest),
            "y" => snap.y = parse_i32(&rest),
            "width" => snap.width = parse_u32(&rest),
            "height" => snap.height = parse_u32(&rest),
            _ => {}
        }
    }
    Ok(snap)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::sync::OnceLock;

    use super::*;

    fn path_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct PathGuard {
        previous: Option<OsString>,
        fake_dir: PathBuf,
    }

    impl PathGuard {
        fn install() -> Result<Self> {
            let fake_dir = std::env::temp_dir()
                .join(format!("yeetnyoink-mmsg-missing-{}", std::process::id()));
            if fake_dir.exists() {
                fs::remove_dir_all(&fake_dir)?;
            }
            fs::create_dir_all(&fake_dir)?;
            let previous = std::env::var_os("PATH");
            // Empty dir should hide binaries from PATH lookup.
            std::env::set_var("PATH", &fake_dir);
            Ok(Self { previous, fake_dir })
        }
    }

    impl Drop for PathGuard {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                std::env::set_var("PATH", previous);
            } else {
                std::env::remove_var("PATH");
            }
            let _ = fs::remove_dir_all(&self.fake_dir);
        }
    }

    fn run_with_fake_path_that_hides_mmsg() -> Result<()> {
        let _guard = path_lock().lock().expect("path lock poisoned");
        let _path = PathGuard::install()?;
        ensure_binary_present()
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
            build_focusdir_dispatch("left"),
            vec!["-d".to_string(), "focusdir,left".to_string()]
        );
        assert_eq!(
            build_exchange_client_dispatch("right"),
            vec!["-d".to_string(), "exchange_client,right".to_string()]
        );
        assert_eq!(
            build_tagmon_dispatch("next"),
            vec!["-d".to_string(), "tagmon,next".to_string()]
        );
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
    fn mangowc_mmsg_spawn_preserves_space_containing_args() {
        assert_eq!(
            build_spawn_dispatch(&[
                "foot".to_string(),
                "--title".to_string(),
                "my terminal title".to_string(),
            ])
            .unwrap(),
            vec![
                "-d".to_string(),
                "spawn,foot,--title,my terminal title".to_string(),
            ]
        );
    }

    #[test]
    fn mangowc_mmsg_spawn_preserves_quote_containing_args() {
        assert_eq!(
            build_spawn_dispatch(&[
                "foot".to_string(),
                "--title".to_string(),
                "don't panic".to_string(),
            ])
            .unwrap(),
            vec![
                "-d".to_string(),
                "spawn,foot,--title,don't panic".to_string(),
            ]
        );
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
    fn mangowc_mmsg_reports_missing_binary_cleanly() {
        let err = run_with_fake_path_that_hides_mmsg().unwrap_err();
        assert!(err.to_string().contains("mmsg"));
    }
}
