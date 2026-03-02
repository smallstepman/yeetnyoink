use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::Args;
use niri_ipc::{Window, WorkspaceReferenceArg};
use serde::{Deserialize, Serialize};

use crate::adapters::window_managers::niri::Niri;

#[derive(Debug, Clone, Args)]
pub struct FocusOrCycleArgs {
    /// Match windows by app_id.
    #[arg(long)]
    pub app_id: Option<String>,
    /// Match windows by title substring (case-insensitive).
    #[arg(long)]
    pub title: Option<String>,
    /// Spawn command if no matching window exists.
    #[arg(long)]
    pub spawn: Option<String>,
    /// Always spawn a new instance (requires --spawn).
    #[arg(long, default_value_t = false)]
    pub new: bool,
    /// Summon behavior: bring to current view and toggle back to origin.
    #[arg(long, default_value_t = false)]
    pub summon: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SummonOrigin {
    workspace_id: u64,
    output: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SummonState {
    windows: HashMap<u64, SummonOrigin>,
}

pub fn run(args: FocusOrCycleArgs) -> Result<()> {
    if args.app_id.is_none() && args.title.is_none() {
        bail!("focus-or-cycle requires --app-id and/or --title");
    }

    let mut niri = Niri::connect()?;

    if args.new {
        let spawn = args
            .spawn
            .as_ref()
            .context("--new requires --spawn '<command>'")?;
        return niri.spawn_sh(spawn.clone());
    }

    let windows = niri.windows()?;
    let focused_id = windows.iter().find(|w| w.is_focused).map(|w| w.id);

    let app_id = args.app_id.as_deref();
    let title = args.title.as_deref();
    let mut matches: Vec<Window> = windows
        .iter()
        .filter(|w| window_matches(w, app_id, title))
        .cloned()
        .collect();

    if matches.is_empty() {
        if let Some(spawn) = args.spawn {
            return niri.spawn_sh(spawn);
        }
        bail!("no matching windows found and no --spawn provided");
    }

    matches.sort_by(|a, b| focus_sort_key(b).cmp(&focus_sort_key(a)));
    let target_idx = focused_id
        .and_then(|id| matches.iter().position(|w| w.id == id))
        .map(|idx| (idx + 1) % matches.len())
        .unwrap_or(0);
    let target = matches[target_idx].clone();

    if args.summon {
        summon_or_return(&mut niri, &target, &windows)?;
        return Ok(());
    }

    niri.focus_window_by_id(target.id)
}

fn window_matches(window: &Window, app_id: Option<&str>, title: Option<&str>) -> bool {
    if let Some(app_id) = app_id {
        if window.app_id.as_deref() != Some(app_id) {
            return false;
        }
    }
    if let Some(title) = title {
        let Some(window_title) = window.title.as_deref() else {
            return false;
        };
        if !window_title.to_lowercase().contains(&title.to_lowercase()) {
            return false;
        }
    }
    true
}

fn focus_sort_key(window: &Window) -> (u64, u32, u64) {
    let (secs, nanos) = window
        .focus_timestamp
        .map(|ts| (ts.secs, ts.nanos))
        .unwrap_or((0, 0));
    (secs, nanos, window.id)
}

fn summon_or_return(niri: &mut Niri, target: &Window, all_windows: &[Window]) -> Result<()> {
    let workspaces = niri.workspaces()?;
    let focused_workspace = workspaces
        .iter()
        .find(|ws| ws.is_focused)
        .cloned()
        .context("no focused workspace found")?;

    let workspaces_by_id: HashMap<u64, _> = workspaces.iter().map(|ws| (ws.id, ws)).collect();
    let mut state = load_summon_state()?;

    let live_window_ids: HashSet<u64> = all_windows.iter().map(|w| w.id).collect();
    state
        .windows
        .retain(|window_id, _| live_window_ids.contains(window_id));

    if target.is_focused {
        if let Some(origin) = state.windows.remove(&target.id) {
            niri.move_window_to_workspace(
                target.id,
                WorkspaceReferenceArg::Id(origin.workspace_id),
                false,
            )?;
            if let Some(output) = origin.output {
                niri.move_window_to_monitor(target.id, output)?;
            }
            save_summon_state(&state)?;
            return Ok(());
        }
    }

    if target.workspace_id != Some(focused_workspace.id) {
        state.windows.entry(target.id).or_insert_with(|| {
            let origin_output = target
                .workspace_id
                .and_then(|workspace_id| workspaces_by_id.get(&workspace_id))
                .and_then(|ws| ws.output.clone());
            SummonOrigin {
                workspace_id: target.workspace_id.unwrap_or(focused_workspace.id),
                output: origin_output,
            }
        });

        niri.move_window_to_workspace(
            target.id,
            WorkspaceReferenceArg::Id(focused_workspace.id),
            false,
        )?;
        if let Some(output) = focused_workspace.output.clone() {
            niri.move_window_to_monitor(target.id, output)?;
        }
        save_summon_state(&state)?;
    }

    niri.focus_window_by_id(target.id)
}

fn summon_state_path() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("niri-deep").join("summon-state.json")
}

fn load_summon_state() -> Result<SummonState> {
    let path = summon_state_path();
    if !path.exists() {
        return Ok(SummonState::default());
    }

    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read summon state file: {}", path.display()))?;
    let state: SummonState = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse summon state file: {}", path.display()))?;
    Ok(state)
}

fn save_summon_state(state: &SummonState) -> Result<()> {
    let path = summon_state_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create state directory: {}", parent.display()))?;
    }

    let serialized = serde_json::to_string(state).context("failed to serialize summon state")?;
    fs::write(&path, serialized)
        .with_context(|| format!("failed to write summon state file: {}", path.display()))?;
    Ok(())
}
