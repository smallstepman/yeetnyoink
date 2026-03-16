use anyhow::{bail, Result};

use crate::config::{self, BrowserFocusAction, BrowserMoveAction};
use crate::engine::browser_native::BrowserTabState;
use crate::engine::contracts::MoveDecision;
use crate::engine::topology::Direction;

enum BrowserTabResolution {
    PassThrough,
    Boundary,
    Internal {
        direction: Direction,
        repetitions: usize,
    },
}

impl BrowserTabResolution {
    fn from_steps(direction: Direction, repetitions: usize) -> Self {
        if repetitions == 0 {
            Self::Boundary
        } else {
            Self::Internal {
                direction,
                repetitions,
            }
        }
    }
}

pub(crate) fn focus_routes_through_wm(aliases: &[&str], direction: Direction) -> bool {
    matches!(
        config::browser_focus_action_for(aliases, direction),
        BrowserFocusAction::Ignore
    )
}

pub(crate) fn move_routes_through_wm(aliases: &[&str], direction: Direction) -> bool {
    matches!(
        config::browser_move_action_for(aliases, direction),
        BrowserMoveAction::Ignore
    )
}

pub(crate) fn can_focus(aliases: &[&str], state: BrowserTabState, direction: Direction) -> bool {
    matches!(
        focus_resolution(aliases, state, direction),
        BrowserTabResolution::Internal { .. }
    )
}

pub(crate) fn move_decision(
    aliases: &[&str],
    state: BrowserTabState,
    direction: Direction,
) -> MoveDecision {
    if state.tab_count <= 1 {
        return MoveDecision::Passthrough;
    }
    match move_resolution(aliases, state, direction) {
        BrowserTabResolution::PassThrough => MoveDecision::Passthrough,
        BrowserTabResolution::Boundary => MoveDecision::TearOut,
        BrowserTabResolution::Internal { .. } => MoveDecision::Internal,
    }
}

pub(crate) fn execute_focus(
    adapter_name: &str,
    aliases: &[&str],
    state: BrowserTabState,
    direction: Direction,
    mut step_focus: impl FnMut(Direction) -> Result<()>,
) -> Result<()> {
    match focus_resolution(aliases, state, direction) {
        BrowserTabResolution::PassThrough => bail!(
            "{adapter_name} routes {direction} focus through the WM under current browser config"
        ),
        BrowserTabResolution::Boundary => {
            bail!("{adapter_name} cannot focus {direction} inside the configured tab strip")
        }
        BrowserTabResolution::Internal {
            direction,
            repetitions,
        } => repeat(direction, repetitions, &mut step_focus),
    }
}

pub(crate) fn execute_move(
    adapter_name: &str,
    aliases: &[&str],
    state: BrowserTabState,
    direction: Direction,
    mut step_move: impl FnMut(Direction) -> Result<()>,
) -> Result<()> {
    match move_resolution(aliases, state, direction) {
        BrowserTabResolution::PassThrough => bail!(
            "{adapter_name} routes {direction} move through the WM under current browser config"
        ),
        BrowserTabResolution::Boundary => {
            bail!("{adapter_name} cannot move the current tab {direction}")
        }
        BrowserTabResolution::Internal {
            direction,
            repetitions,
        } => repeat(direction, repetitions, &mut step_move),
    }
}

fn repeat(
    direction: Direction,
    repetitions: usize,
    action: &mut impl FnMut(Direction) -> Result<()>,
) -> Result<()> {
    for _ in 0..repetitions {
        action(direction)?;
    }
    Ok(())
}

fn focus_resolution(
    aliases: &[&str],
    state: BrowserTabState,
    direction: Direction,
) -> BrowserTabResolution {
    match config::browser_focus_action_for(aliases, direction) {
        BrowserFocusAction::Ignore => BrowserTabResolution::PassThrough,
        BrowserFocusAction::FocusPreviousTab => BrowserTabResolution::from_steps(
            Direction::West,
            usize::from(state.active_tab_index > 0),
        ),
        BrowserFocusAction::FocusNextTab => BrowserTabResolution::from_steps(
            Direction::East,
            usize::from(state.active_tab_index + 1 < state.tab_count),
        ),
        BrowserFocusAction::FocusFirstTab => {
            BrowserTabResolution::from_steps(Direction::West, state.active_tab_index)
        }
        BrowserFocusAction::FocusLastTab => BrowserTabResolution::from_steps(
            Direction::East,
            state.tab_count.saturating_sub(state.active_tab_index + 1),
        ),
    }
}

fn move_resolution(
    aliases: &[&str],
    state: BrowserTabState,
    direction: Direction,
) -> BrowserTabResolution {
    match config::browser_move_action_for(aliases, direction) {
        BrowserMoveAction::Ignore => BrowserTabResolution::PassThrough,
        BrowserMoveAction::MoveTabBackward => BrowserTabResolution::from_steps(
            Direction::West,
            usize::from(state.active_tab_index > move_lower_bound(state)),
        ),
        BrowserMoveAction::MoveTabForward => BrowserTabResolution::from_steps(
            Direction::East,
            usize::from(state.active_tab_index < move_upper_bound(state)),
        ),
        BrowserMoveAction::MoveTabToFirstPosition => BrowserTabResolution::from_steps(
            Direction::West,
            state
                .active_tab_index
                .saturating_sub(move_lower_bound(state)),
        ),
        BrowserMoveAction::MoveTabToLastPosition => BrowserTabResolution::from_steps(
            Direction::East,
            move_upper_bound(state).saturating_sub(state.active_tab_index),
        ),
    }
}

fn move_lower_bound(state: BrowserTabState) -> usize {
    if state.active_tab_pinned {
        0
    } else {
        state.pinned_tab_count
    }
}

fn move_upper_bound(state: BrowserTabState) -> usize {
    if state.active_tab_pinned {
        state.pinned_tab_count.saturating_sub(1)
    } else {
        state.tab_count.saturating_sub(1)
    }
}
