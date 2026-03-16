use std::collections::BTreeMap;

use zellij_tile::prelude::*;

const TAB_RESTORE_RETRY_SECONDS: f64 = 0.2;
const TAB_RESTORE_GRACE_SECONDS: f64 = 0.6;
const TAB_RESTORE_MAX_ATTEMPTS: u8 = 20;

#[derive(Clone, Copy)]
struct PendingTabRestore {
    source_tab_index: u32,
    baseline_client_count: usize,
    attempts_left: u8,
    client_seen: bool,
}

#[derive(Default)]
struct YeetAndYoinkZellijBreakPlugin {
    pane_manifest: Option<PaneManifest>,
    tabs: Vec<TabInfo>,
    clients: Vec<ClientInfo>,
    pending_tab_restore: Option<PendingTabRestore>,
}

register_plugin!(YeetAndYoinkZellijBreakPlugin);

impl ZellijPlugin for YeetAndYoinkZellijBreakPlugin {
    fn load(&mut self, _configuration: BTreeMap<String, String>) {
        set_selectable(false);
        request_permission(&[
            PermissionType::ChangeApplicationState,
            PermissionType::ReadApplicationState,
            PermissionType::ReadCliPipes,
        ]);
        subscribe(&[
            EventType::PaneUpdate,
            EventType::TabUpdate,
            EventType::ListClients,
            EventType::PermissionRequestResult,
            EventType::Timer,
        ]);
        list_clients();
    }

    fn pipe(&mut self, pipe_message: PipeMessage) -> bool {
        let action = pipe_message
            .args
            .get("action")
            .map(String::as_str)
            .or(pipe_message.payload.as_deref())
            .unwrap_or("break");
        if action == "query-pane-id" {
            if let Some(source) = cli_pipe_source_name(&pipe_message) {
                if let Some(pane_id) = self.focused_pane_id_from_state() {
                    cli_pipe_output(source, &pane_id_to_string(&pane_id));
                } else {
                    list_clients();
                }
            }
            return false;
        }
        if action == "merge" {
            let source_pane_id = pipe_message
                .args
                .get("source_pane_id")
                .or_else(|| pipe_message.args.get("pane_id"))
                .and_then(|value| parse_pane_id(value))
                .or_else(|| self.focused_pane_id_from_state());
            let target_tab_index = pipe_message
                .args
                .get("target_tab_index")
                .and_then(|value| value.parse::<usize>().ok())
                .or_else(|| self.active_tab_index_from_state());
            let (Some(source_pane_id), Some(target_tab_index)) = (source_pane_id, target_tab_index)
            else {
                list_clients();
                return false;
            };
            break_panes_to_tab_with_index(&[source_pane_id], target_tab_index, false);
            return false;
        }

        let pane_id = pipe_message
            .args
            .get("pane_id")
            .and_then(|value| parse_pane_id(value))
            .or_else(|| pipe_message.payload.as_deref().and_then(parse_pane_id))
            .or_else(|| self.focused_pane_id_from_state());
        let Some(pane_id) = pane_id else {
            list_clients();
            return false;
        };

        let focus_new_tab = pipe_message
            .args
            .get("focus_new_tab")
            .map(|value| value != "false")
            .unwrap_or(true);
        let new_tab_name = pipe_message.args.get("new_tab_name").cloned();
        let source_tab_index = pipe_message
            .args
            .get("source_tab_index")
            .and_then(|value| value.parse::<u32>().ok())
            .or_else(|| {
                self.active_tab_index_from_state()
                    .and_then(|index| u32::try_from(index).ok())
            });
        let source_client_count = pipe_message
            .args
            .get("source_client_count")
            .and_then(|value| value.parse::<usize>().ok())
            .or_else(|| (!self.clients.is_empty()).then_some(self.clients.len()))
            .unwrap_or(1)
            .max(1);
        break_panes_to_new_tab(&[pane_id], new_tab_name, focus_new_tab);
        if focus_new_tab {
            if let Some(source_tab_index) = source_tab_index {
                self.pending_tab_restore = Some(PendingTabRestore {
                    source_tab_index,
                    baseline_client_count: source_client_count,
                    attempts_left: TAB_RESTORE_MAX_ATTEMPTS,
                    client_seen: false,
                });
                set_timeout(TAB_RESTORE_RETRY_SECONDS);
                list_clients();
            }
        } else {
            self.pending_tab_restore = None;
        }
        false
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::PaneUpdate(pane_manifest) => self.pane_manifest = Some(pane_manifest),
            Event::TabUpdate(tabs) => self.tabs = tabs,
            Event::ListClients(clients) => {
                self.clients = clients;
                self.maybe_restore_source_tab(false);
            }
            Event::PermissionRequestResult(_) => list_clients(),
            Event::Timer(_) => self.maybe_restore_source_tab(true),
            _ => {}
        }
        false
    }
}

fn parse_pane_id(raw: &str) -> Option<PaneId> {
    if let Some(value) = raw.strip_prefix("terminal_") {
        let pane_id = value.parse::<u32>().ok()?;
        return (pane_id > 0).then_some(PaneId::Terminal(pane_id));
    }
    if let Some(value) = raw.strip_prefix("plugin_") {
        let pane_id = value.parse::<u32>().ok()?;
        return (pane_id > 0).then_some(PaneId::Plugin(pane_id));
    }
    let pane_id = raw.parse::<u32>().ok()?;
    (pane_id > 0).then_some(PaneId::Terminal(pane_id))
}

fn pane_id_to_string(pane_id: &PaneId) -> String {
    match pane_id {
        PaneId::Terminal(id) => format!("terminal_{id}"),
        PaneId::Plugin(id) => format!("plugin_{id}"),
    }
}

fn cli_pipe_source_name(pipe_message: &PipeMessage) -> Option<&str> {
    match &pipe_message.source {
        PipeSource::Cli(name) => Some(name.as_str()),
        _ => None,
    }
}

impl YeetAndYoinkZellijBreakPlugin {
    fn maybe_restore_source_tab(&mut self, from_timer: bool) {
        let Some(mut pending) = self.pending_tab_restore else {
            return;
        };

        let active_tab_index = self
            .active_tab_index_from_state()
            .and_then(|index| u32::try_from(index).ok());
        if active_tab_index == Some(pending.source_tab_index) {
            self.pending_tab_restore = None;
            return;
        }

        let required_client_count = pending.baseline_client_count.saturating_add(1);
        if !pending.client_seen && self.clients.len() >= required_client_count {
            pending.client_seen = true;
            self.pending_tab_restore = Some(pending);
            set_timeout(TAB_RESTORE_GRACE_SECONDS);
            return;
        }

        let should_restore_now = if pending.client_seen {
            from_timer
        } else {
            pending.attempts_left <= 1
        };
        if should_restore_now {
            switch_tab_to(pending.source_tab_index);
            self.pending_tab_restore = None;
            return;
        }

        if pending.client_seen {
            self.pending_tab_restore = Some(pending);
            return;
        }

        pending.attempts_left -= 1;
        self.pending_tab_restore = Some(pending);
        set_timeout(TAB_RESTORE_RETRY_SECONDS);
        list_clients();
    }

    fn focused_pane_id_from_state(&self) -> Option<PaneId> {
        if let Some(pane_id) = self
            .clients
            .iter()
            .find(|client| client.is_current_client && !pane_id_is_zero(&client.pane_id))
            .map(|client| client.pane_id.clone())
        {
            return Some(pane_id);
        }
        if let Some(pane_id) = self
            .clients
            .iter()
            .find(|client| !pane_id_is_zero(&client.pane_id))
            .map(|client| client.pane_id.clone())
        {
            return Some(pane_id);
        }
        let tab_position = self
            .tabs
            .iter()
            .find(|tab| tab.active)
            .map(|tab| tab.position)?;
        let pane_manifest = self.pane_manifest.as_ref()?;
        if let Some(pane_info) = get_focused_pane(tab_position, pane_manifest) {
            if !pane_info.is_plugin && pane_info.id > 0 {
                return Some(PaneId::Terminal(pane_info.id));
            }
        }
        pane_manifest
            .panes
            .get(&tab_position)
            .and_then(|panes| {
                panes
                    .iter()
                    .find(|pane| pane.is_focused && !pane.is_plugin && pane.id > 0)
            })
            .map(|pane| PaneId::Terminal(pane.id))
    }

    fn active_tab_index_from_state(&self) -> Option<usize> {
        self.tabs
            .iter()
            .find(|tab| tab.active)
            .map(|tab| tab.position)
    }
}

fn pane_id_is_zero(pane_id: &PaneId) -> bool {
    matches!(pane_id, PaneId::Terminal(0) | PaneId::Plugin(0))
}
