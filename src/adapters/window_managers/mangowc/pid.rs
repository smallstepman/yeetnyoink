use std::collections::HashSet;

use crate::adapters::apps::{self, alacritty, foot, ghostty, kitty, wezterm};
use crate::engine::runtime::{self, ProcessId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProcessCandidate {
    pub pid: ProcessId,
    pub comm: String,
}

pub(crate) trait FocusedPidResolver: Send {
    fn resolve_focused_terminal_pid(
        &self,
        app_id: Option<&str>,
        title: Option<&str>,
    ) -> Option<ProcessId>;
}

#[derive(Debug, Default)]
pub(crate) struct RuntimeFocusedPidResolver;

impl FocusedPidResolver for RuntimeFocusedPidResolver {
    fn resolve_focused_terminal_pid(
        &self,
        app_id: Option<&str>,
        title: Option<&str>,
    ) -> Option<ProcessId> {
        resolve_focused_terminal_pid(
            app_id,
            title,
            runtime::all_pids().into_iter().filter_map(|raw_pid| {
                Some(ProcessCandidate {
                    pid: ProcessId::new(raw_pid)?,
                    comm: runtime::process_comm(raw_pid)?,
                })
            }),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusedPidMatch {
    Strong(ProcessId),
    Ambiguous,
    None,
}

pub(crate) fn resolve_focused_terminal_pid<I>(
    app_id: Option<&str>,
    _title: Option<&str>,
    processes: I,
) -> Option<ProcessId>
where
    I: IntoIterator<Item = ProcessCandidate>,
{
    match classify_focused_terminal_pid(app_id, processes) {
        FocusedPidMatch::Strong(pid) => Some(pid),
        FocusedPidMatch::Ambiguous | FocusedPidMatch::None => None,
    }
}

fn classify_focused_terminal_pid<I>(app_id: Option<&str>, processes: I) -> FocusedPidMatch
where
    I: IntoIterator<Item = ProcessCandidate>,
{
    let Some(expected_names) = app_id.and_then(expected_process_names_for_app_id) else {
        return FocusedPidMatch::None;
    };

    let matches: HashSet<ProcessId> = processes
        .into_iter()
        .filter_map(|candidate| {
            let comm = runtime::normalize_process_name(&candidate.comm);
            expected_names
                .iter()
                .any(|expected| *expected == comm)
                .then_some(candidate.pid)
        })
        .collect();

    match matches.len() {
        0 => FocusedPidMatch::None,
        1 => FocusedPidMatch::Strong(
            *matches
                .iter()
                .next()
                .expect("single strong pid match should exist"),
        ),
        _ => FocusedPidMatch::Ambiguous,
    }
}

fn expected_process_names_for_app_id(app_id: &str) -> Option<&'static [&'static str]> {
    if !apps::TERMINAL_HOSTS
        .iter()
        .any(|host| host.app_ids.contains(&app_id))
    {
        return None;
    }

    const FOOT_PROCESS_NAMES: &[&str] = &["foot", "footclient"];
    const KITTY_PROCESS_NAMES: &[&str] = &["kitty"];
    const WEZTERM_PROCESS_NAMES: &[&str] = &["wezterm", "wezterm-gui"];
    const ALACRITTY_PROCESS_NAMES: &[&str] = &["Alacritty", "alacritty"];
    const GHOSTTY_PROCESS_NAMES: &[&str] = &["ghostty"];

    if foot::APP_IDS.contains(&app_id) {
        Some(FOOT_PROCESS_NAMES)
    } else if kitty::APP_IDS.contains(&app_id) {
        Some(KITTY_PROCESS_NAMES)
    } else if wezterm::APP_IDS.contains(&app_id) {
        Some(WEZTERM_PROCESS_NAMES)
    } else if alacritty::APP_IDS.contains(&app_id) {
        Some(ALACRITTY_PROCESS_NAMES)
    } else if ghostty::APP_IDS.contains(&app_id) {
        Some(GHOSTTY_PROCESS_NAMES)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(pid: u32, comm: &str) -> ProcessCandidate {
        ProcessCandidate {
            pid: ProcessId::new(pid).expect("candidate pid should be non-zero"),
            comm: comm.to_string(),
        }
    }

    #[test]
    fn mangowc_pid_unique_terminal_host_candidate_is_accepted() {
        let pid =
            resolve_focused_terminal_pid(Some("foot"), Some("shell"), [candidate(4242, "foot")]);
        assert_eq!(pid, ProcessId::new(4242));
    }

    #[test]
    fn mangowc_pid_ambiguous_terminal_host_candidates_return_none() {
        let pid = resolve_focused_terminal_pid(
            Some("foot"),
            Some("shell"),
            [candidate(4242, "foot"), candidate(4343, "footclient")],
        );
        assert_eq!(pid, None);
    }

    #[test]
    fn mangowc_pid_unknown_non_terminal_app_id_returns_none() {
        let pid =
            resolve_focused_terminal_pid(Some("firefox"), Some("shell"), [candidate(4242, "foot")]);
        assert_eq!(pid, None);
    }
}
