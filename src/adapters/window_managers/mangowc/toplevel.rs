use std::collections::HashMap;

use anyhow::{anyhow, bail, Context, Result};

use crate::engine::runtime::ProcessId;
use crate::engine::wm::{FocusedWindowRecord, WindowRecord};

const ACTIVATED_STATE: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToplevelSnapshotEntry {
    pub handle_id: u64,
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub pid: Option<ProcessId>,
    pub activated: bool,
}

#[derive(Debug, Default)]
pub struct ToplevelStore {
    next_window_id: u64,
    handle_to_window_id: HashMap<u64, u64>,
    window_id_to_handle: HashMap<u64, u64>,
    windows: Vec<WindowRecord>,
}

impl ToplevelStore {
    pub fn replace_snapshot(&mut self, snapshot: &[ToplevelSnapshotEntry]) {
        let mut new_handle_to_window_id = HashMap::with_capacity(snapshot.len());
        let mut new_window_id_to_handle = HashMap::with_capacity(snapshot.len());
        let mut windows = Vec::with_capacity(snapshot.len());

        for (idx, entry) in snapshot.iter().enumerate() {
            let id = self
                .handle_to_window_id
                .get(&entry.handle_id)
                .copied()
                .unwrap_or_else(|| {
                    self.next_window_id = self.next_window_id.saturating_add(1);
                    self.next_window_id
                });
            new_handle_to_window_id.insert(entry.handle_id, id);
            new_window_id_to_handle.insert(id, entry.handle_id);
            windows.push(WindowRecord {
                id,
                app_id: entry.app_id.clone(),
                title: entry.title.clone(),
                pid: entry.pid,
                is_focused: entry.activated,
                original_tile_index: idx,
            });
        }

        self.handle_to_window_id = new_handle_to_window_id;
        self.window_id_to_handle = new_window_id_to_handle;
        self.windows = windows;
    }

    pub fn windows(&self) -> &[WindowRecord] {
        &self.windows
    }

    pub fn focused_window(&self) -> Result<FocusedWindowRecord> {
        let focused = self
            .windows
            .iter()
            .find(|window| window.is_focused)
            .ok_or_else(|| anyhow!("mangowc: no activated foreign toplevel window"))?;
        Ok(FocusedWindowRecord {
            id: focused.id,
            app_id: focused.app_id.clone(),
            title: focused.title.clone(),
            pid: focused.pid,
            original_tile_index: focused.original_tile_index,
        })
    }

    pub fn handle_for_window_id(&self, id: u64) -> Result<u64> {
        self.window_id_to_handle
            .get(&id)
            .copied()
            .ok_or_else(|| anyhow!("mangowc: stale window id {id}"))
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RegistrySnapshot {
    pub has_wl_seat: bool,
    pub has_foreign_toplevel_manager: bool,
}

#[derive(Debug, Default)]
struct SessionState {
    registry: RegistrySnapshot,
    seat: Option<wayland_client::protocol::wl_seat::WlSeat>,
    manager: Option<ForeignToplevelManager>,
    next_handle_id: u64,
    protocol_to_handle_id: HashMap<u32, u64>,
    handles: HashMap<u64, ForeignToplevelHandle>,
    snapshots: HashMap<u64, ToplevelSnapshotEntry>,
}

#[derive(Debug)]
pub struct ForeignToplevelSession {
    connection: wayland_client::Connection,
    event_queue: wayland_client::EventQueue<SessionState>,
    state: SessionState,
    seat: wayland_client::protocol::wl_seat::WlSeat,
}

type ForeignToplevelManager = wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_manager_v1::ZwlrForeignToplevelManagerV1;
type ForeignToplevelHandle = wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_handle_v1::ZwlrForeignToplevelHandleV1;

impl SessionState {
    fn register_protocol_handle(&mut self, protocol_id: u32) -> u64 {
        if let Some(handle_id) = self.protocol_to_handle_id.get(&protocol_id).copied() {
            return handle_id;
        }
        self.next_handle_id = self.next_handle_id.saturating_add(1);
        let handle_id = self.next_handle_id;
        self.protocol_to_handle_id.insert(protocol_id, handle_id);
        handle_id
    }

    fn handle_id_for_protocol(&self, protocol_id: u32) -> Option<u64> {
        self.protocol_to_handle_id.get(&protocol_id).copied()
    }

    fn unregister_protocol_handle(&mut self, protocol_id: u32) -> Option<u64> {
        self.protocol_to_handle_id.remove(&protocol_id)
    }
}

impl ForeignToplevelSession {
    pub fn connect() -> Result<Self> {
        if std::env::var_os("WAYLAND_DISPLAY").is_none() {
            bail!("mangowc: missing WAYLAND_DISPLAY");
        }

        let connection = wayland_client::Connection::connect_to_env()
            .context("mangowc: failed to connect to WAYLAND_DISPLAY")?;
        let display = connection.display();
        let mut event_queue = connection.new_event_queue();
        let qh = event_queue.handle();
        let _registry = display.get_registry(&qh, ());

        let mut state = SessionState::default();
        event_queue
            .roundtrip(&mut state)
            .context("mangowc: failed initial wayland registry roundtrip")?;

        Self::ensure_required_globals(state.registry)?;

        let seat = state
            .seat
            .clone()
            .ok_or_else(|| anyhow!("mangowc: missing wl_seat"))?;

        if state.manager.is_none() {
            bail!("mangowc: missing zwlr_foreign_toplevel_manager_v1");
        }

        Ok(Self {
            connection,
            event_queue,
            state,
            seat,
        })
    }

    pub fn refresh_store(&mut self, store: &mut ToplevelStore) -> Result<()> {
        self.event_queue
            .roundtrip(&mut self.state)
            .context("mangowc: failed foreign-toplevel roundtrip")?;
        let mut snapshot: Vec<ToplevelSnapshotEntry> =
            self.state.snapshots.values().cloned().collect();
        snapshot.sort_by_key(|entry| entry.handle_id);
        store.replace_snapshot(&snapshot);
        Ok(())
    }

    pub fn activate_window_by_id(&mut self, store: &ToplevelStore, id: u64) -> Result<()> {
        let handle_id = store.handle_for_window_id(id)?;
        let handle = self
            .state
            .handles
            .get(&handle_id)
            .ok_or_else(|| anyhow!("mangowc: stale window id {id}"))?;
        handle.activate(&self.seat);
        self.connection
            .flush()
            .context("mangowc: failed to flush activate request")?;
        Ok(())
    }

    pub fn close_window_by_id(&mut self, store: &ToplevelStore, id: u64) -> Result<()> {
        let handle_id = store.handle_for_window_id(id)?;
        let handle = self
            .state
            .handles
            .get(&handle_id)
            .ok_or_else(|| anyhow!("mangowc: stale window id {id}"))?;
        handle.close();
        self.connection
            .flush()
            .context("mangowc: failed to flush close request")?;
        Ok(())
    }

    fn ensure_required_globals(registry: RegistrySnapshot) -> Result<()> {
        if !registry.has_foreign_toplevel_manager {
            bail!("mangowc: missing zwlr_foreign_toplevel_manager_v1");
        }
        if !registry.has_wl_seat {
            bail!("mangowc: missing wl_seat");
        }
        Ok(())
    }
}

impl wayland_client::Dispatch<wayland_client::protocol::wl_registry::WlRegistry, ()>
    for SessionState
{
    fn event(
        state: &mut Self,
        registry: &wayland_client::protocol::wl_registry::WlRegistry,
        event: wayland_client::protocol::wl_registry::Event,
        _: &(),
        _: &wayland_client::Connection,
        qhandle: &wayland_client::QueueHandle<Self>,
    ) {
        use wayland_client::protocol::wl_registry::Event;
        match event {
            Event::Global {
                name,
                interface,
                version,
            } => match interface.as_str() {
                "wl_seat" => {
                    let seat_version = version.min(8);
                    let seat = registry.bind::<wayland_client::protocol::wl_seat::WlSeat, _, _>(
                        name,
                        seat_version,
                        qhandle,
                        (),
                    );
                    state.registry.has_wl_seat = true;
                    state.seat = Some(seat);
                }
                "zwlr_foreign_toplevel_manager_v1" => {
                    let manager = registry.bind::<ForeignToplevelManager, _, _>(
                        name,
                        version.min(3),
                        qhandle,
                        (),
                    );
                    state.registry.has_foreign_toplevel_manager = true;
                    state.manager = Some(manager);
                }
                _ => {}
            },
            Event::GlobalRemove { .. } => {}
            _ => {}
        }
    }
}

impl wayland_client::Dispatch<wayland_client::protocol::wl_seat::WlSeat, ()> for SessionState {
    fn event(
        _state: &mut Self,
        _proxy: &wayland_client::protocol::wl_seat::WlSeat,
        _event: wayland_client::protocol::wl_seat::Event,
        _data: &(),
        _conn: &wayland_client::Connection,
        _qhandle: &wayland_client::QueueHandle<Self>,
    ) {
    }
}

impl wayland_client::Dispatch<ForeignToplevelManager, ()> for SessionState {
    fn event(
        state: &mut Self,
        _manager: &ForeignToplevelManager,
        event: wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_manager_v1::Event,
        _data: &(),
        _conn: &wayland_client::Connection,
        _qhandle: &wayland_client::QueueHandle<Self>,
    ) {
        use wayland_client::Proxy;
        use wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_manager_v1::Event;
        match event {
            Event::Toplevel { toplevel } => {
                let handle_id = state.register_protocol_handle(toplevel.id().protocol_id());
                state.handles.insert(handle_id, toplevel);
                state
                    .snapshots
                    .entry(handle_id)
                    .or_insert_with(|| ToplevelSnapshotEntry {
                        handle_id,
                        app_id: None,
                        title: None,
                        pid: None,
                        activated: false,
                    });
            }
            Event::Finished => {}
            _ => {}
        }
    }

    wayland_client::event_created_child!(SessionState, ForeignToplevelManager, [
        0 => (ForeignToplevelHandle, ())
    ]);
}

impl wayland_client::Dispatch<ForeignToplevelHandle, ()> for SessionState {
    fn event(
        state: &mut Self,
        handle: &ForeignToplevelHandle,
        event: wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_handle_v1::Event,
        _data: &(),
        _conn: &wayland_client::Connection,
        _qhandle: &wayland_client::QueueHandle<Self>,
    ) {
        use wayland_client::Proxy;
        use wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_handle_v1::Event;

        let Some(handle_id) = state.handle_id_for_protocol(handle.id().protocol_id()) else {
            return;
        };
        match event {
            Event::Title { title } => {
                let entry = state
                    .snapshots
                    .entry(handle_id)
                    .or_insert_with(|| default_snapshot(handle_id));
                entry.title = Some(title);
            }
            Event::AppId { app_id } => {
                let entry = state
                    .snapshots
                    .entry(handle_id)
                    .or_insert_with(|| default_snapshot(handle_id));
                entry.app_id = Some(app_id);
            }
            Event::State { state: values } => {
                let activated = values
                    .chunks_exact(4)
                    .map(|bytes| u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
                    .any(|raw| raw == ACTIVATED_STATE);
                let entry = state
                    .snapshots
                    .entry(handle_id)
                    .or_insert_with(|| default_snapshot(handle_id));
                entry.activated = activated;
            }
            Event::Done => {}
            Event::Closed => {
                if let Some(handle_id) = state.unregister_protocol_handle(handle.id().protocol_id())
                {
                    state.snapshots.remove(&handle_id);
                    state.handles.remove(&handle_id);
                }
            }
            _ => {}
        }
    }
}

impl wayland_client::Dispatch<wayland_client::protocol::wl_output::WlOutput, ()> for SessionState {
    fn event(
        _state: &mut Self,
        _proxy: &wayland_client::protocol::wl_output::WlOutput,
        _event: wayland_client::protocol::wl_output::Event,
        _data: &(),
        _conn: &wayland_client::Connection,
        _qhandle: &wayland_client::QueueHandle<Self>,
    ) {
    }
}

fn default_snapshot(handle_id: u64) -> ToplevelSnapshotEntry {
    ToplevelSnapshotEntry {
        handle_id,
        app_id: None,
        title: None,
        pid: None,
        activated: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mangowc_toplevel_store_maintains_stable_synthetic_ids_for_repeated_handles() {
        let mut store = ToplevelStore::default();
        let first = vec![
            ToplevelSnapshotEntry {
                handle_id: 10,
                app_id: Some("foot".to_string()),
                title: Some("left".to_string()),
                pid: None,
                activated: true,
            },
            ToplevelSnapshotEntry {
                handle_id: 20,
                app_id: Some("wezterm".to_string()),
                title: Some("right".to_string()),
                pid: None,
                activated: false,
            },
        ];

        store.replace_snapshot(&first);
        let first_id = store.windows()[0].id;

        let second = vec![
            ToplevelSnapshotEntry {
                handle_id: 10,
                app_id: Some("foot".to_string()),
                title: Some("left-updated".to_string()),
                pid: None,
                activated: true,
            },
            ToplevelSnapshotEntry {
                handle_id: 20,
                app_id: Some("wezterm".to_string()),
                title: Some("right-updated".to_string()),
                pid: None,
                activated: false,
            },
        ];

        store.replace_snapshot(&second);
        assert_eq!(store.windows()[0].id, first_id);
    }

    #[test]
    fn mangowc_toplevel_store_reports_activated_handle_as_focused_window() {
        let mut store = ToplevelStore::default();
        store.replace_snapshot(&[
            ToplevelSnapshotEntry {
                handle_id: 41,
                app_id: Some("foot".to_string()),
                title: Some("one".to_string()),
                pid: None,
                activated: false,
            },
            ToplevelSnapshotEntry {
                handle_id: 42,
                app_id: Some("kitty".to_string()),
                title: Some("two".to_string()),
                pid: None,
                activated: true,
            },
        ]);

        let focused = store.focused_window().expect("focused window should exist");
        assert_eq!(focused.title.as_deref(), Some("two"));
    }

    #[test]
    fn mangowc_toplevel_store_rejects_stale_window_id_lookup() {
        let mut store = ToplevelStore::default();
        store.replace_snapshot(&[ToplevelSnapshotEntry {
            handle_id: 99,
            app_id: Some("foot".to_string()),
            title: Some("gone".to_string()),
            pid: None,
            activated: true,
        }]);
        let stale_id = store.windows()[0].id;

        store.replace_snapshot(&[]);

        let err = store.handle_for_window_id(stale_id).expect_err("stale id should fail");
        assert!(err.to_string().contains("stale"));
    }

    #[test]
    fn mangowc_toplevel_session_reuses_protocol_id_with_fresh_handle_identity() {
        let mut state = SessionState::default();
        let first = state.register_protocol_handle(17);
        assert_eq!(state.handle_id_for_protocol(17), Some(first));
        assert_eq!(state.unregister_protocol_handle(17), Some(first));

        let second = state.register_protocol_handle(17);
        assert_ne!(second, first);
        assert_eq!(state.handle_id_for_protocol(17), Some(second));
    }

    #[test]
    fn mangowc_toplevel_connect_fails_without_wayland_display() {
        let _guard = crate::utils::env_guard();
        std::env::remove_var("WAYLAND_DISPLAY");

        let err = ForeignToplevelSession::connect().expect_err("missing env should fail");
        assert!(err.to_string().contains("WAYLAND_DISPLAY"));
    }

    #[test]
    fn mangowc_toplevel_connect_fails_when_foreign_toplevel_manager_missing() {
        let err = ForeignToplevelSession::ensure_required_globals(RegistrySnapshot {
            has_wl_seat: true,
            has_foreign_toplevel_manager: false,
        })
        .expect_err("missing manager should fail");
        assert!(err
            .to_string()
            .contains("zwlr_foreign_toplevel_manager_v1"));
    }

    #[test]
    fn mangowc_toplevel_connect_fails_when_wl_seat_missing() {
        let err = ForeignToplevelSession::ensure_required_globals(RegistrySnapshot {
            has_wl_seat: false,
            has_foreign_toplevel_manager: true,
        })
        .expect_err("missing seat should fail");
        assert!(err.to_string().contains("wl_seat"));
    }

    #[test]
    fn mangowc_toplevel_manager_dispatch_registers_child_factory_for_toplevel_event() {
        use std::os::unix::net::UnixStream;

        let (client, _server) = UnixStream::pair().expect("socket pair should exist");
        let connection = wayland_client::Connection::from_socket(client)
            .expect("client connection should construct");
        let event_queue = connection.new_event_queue::<SessionState>();
        let qhandle = event_queue.handle();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            <SessionState as wayland_client::Dispatch<ForeignToplevelManager, ()>>::event_created_child(
                0,
                &qhandle,
            )
        }));

        assert!(
            result.is_ok(),
            "toplevel manager child factory should exist for opcode 0"
        );
    }
}
