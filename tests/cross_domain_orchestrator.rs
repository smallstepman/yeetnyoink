use std::any::TypeId;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Result};

use niri_deep::adapters::window_managers::{
    FocusedWindowView, ResizeIntent, WindowManagerCapabilities, WindowManagerExecution,
    WindowManagerIntrospection, WindowManagerMetadata, WindowRecord,
};
use niri_deep::engine::domain::PaneState;
use niri_deep::engine::domain::{DomainLeafSnapshot, DomainSnapshot, ErasedDomain};
use niri_deep::engine::domain::{EDITOR_DOMAIN_ID, TERMINAL_DOMAIN_ID, WM_DOMAIN_ID};
use niri_deep::engine::orchestrator::{ActionKind, ActionRequest, Orchestrator};
use niri_deep::engine::runtime::ProcessId;
use niri_deep::engine::topology::Direction;
use niri_deep::engine::topology::Rect;

#[derive(Clone, Default)]
struct DomainCounters {
    tear_off_calls: Arc<AtomicUsize>,
    merge_calls: Arc<AtomicUsize>,
}

#[derive(Debug)]
struct NvimPanePayload;

#[derive(Debug)]
struct WeztermPanePayload;

struct FakeDomain {
    id: u64,
    name: &'static str,
    supported_payloads: Vec<TypeId>,
    payload: Option<Box<dyn PaneState>>,
    counters: DomainCounters,
}

impl FakeDomain {
    fn new(
        id: u64,
        name: &'static str,
        supported_payloads: Vec<TypeId>,
        payload: Option<Box<dyn PaneState>>,
        counters: DomainCounters,
    ) -> Self {
        Self {
            id,
            name,
            supported_payloads,
            payload,
            counters,
        }
    }
}

impl ErasedDomain for FakeDomain {
    fn domain_id(&self) -> u64 {
        self.id
    }

    fn domain_name(&self) -> &'static str {
        self.name
    }

    fn rect(&self) -> Rect {
        Rect {
            x: 0,
            y: 0,
            w: 1000,
            h: 1000,
        }
    }

    fn fetch_snapshot(&mut self) -> Result<DomainSnapshot> {
        Ok(DomainSnapshot {
            domain_id: self.id,
            rect: self.rect(),
            leaves: vec![DomainLeafSnapshot {
                id: 1,
                native_id: vec![1],
                rect: self.rect(),
                focused: true,
            }],
        })
    }

    fn supported_payload_types(&self) -> Vec<TypeId> {
        self.supported_payloads.clone()
    }

    fn tear_off(&mut self, _native_id: &[u8]) -> Result<Box<dyn PaneState>> {
        self.counters.tear_off_calls.fetch_add(1, Ordering::Relaxed);
        self.payload
            .take()
            .ok_or_else(|| anyhow!("no payload available"))
    }

    fn merge_in(
        &mut self,
        _target_native_id: &[u8],
        _dir: Direction,
        _payload: Box<dyn PaneState>,
    ) -> Result<Vec<u8>> {
        self.counters.merge_calls.fetch_add(1, Ordering::Relaxed);
        Ok(vec![9, 9, 9])
    }
}

#[derive(Clone, Copy)]
struct FakeFocusedWindow<'a> {
    inner: &'a WindowRecord,
}

impl FocusedWindowView for FakeFocusedWindow<'_> {
    fn id(&self) -> u64 {
        self.inner.id
    }

    fn app_id(&self) -> Option<&str> {
        self.inner.app_id.as_deref()
    }

    fn title(&self) -> Option<&str> {
        self.inner.title.as_deref()
    }

    fn pid(&self) -> Option<ProcessId> {
        self.inner.pid
    }

    fn original_tile_index(&self) -> usize {
        self.inner.original_tile_index
    }
}

struct FakeWindowManager {
    windows: Vec<WindowRecord>,
    move_calls: usize,
}

impl WindowManagerMetadata for FakeWindowManager {
    fn adapter_name(&self) -> &'static str {
        "fake"
    }

    fn capabilities(&self) -> WindowManagerCapabilities {
        WindowManagerCapabilities::none()
    }
}

impl WindowManagerIntrospection for FakeWindowManager {
    type FocusedWindow<'a>
        = FakeFocusedWindow<'a>
    where
        Self: 'a;

    fn with_focused_window<R>(
        &mut self,
        visit: impl for<'a> FnOnce(Self::FocusedWindow<'a>) -> Result<R>,
    ) -> Result<R> {
        let focused = self
            .windows
            .iter()
            .find(|window| window.is_focused)
            .ok_or_else(|| anyhow!("no focused window"))?;
        visit(FakeFocusedWindow { inner: focused })
    }

    fn windows(&mut self) -> Result<Vec<WindowRecord>> {
        Ok(self.windows.clone())
    }
}

impl WindowManagerExecution for FakeWindowManager {
    fn focus_direction(&mut self, _direction: Direction) -> Result<()> {
        if self.windows.len() < 2 {
            return Ok(());
        }
        let focused_idx = self
            .windows
            .iter()
            .position(|window| window.is_focused)
            .ok_or_else(|| anyhow!("no focused window"))?;
        let target_idx = if focused_idx + 1 < self.windows.len() {
            focused_idx + 1
        } else {
            focused_idx.saturating_sub(1)
        };
        for (idx, window) in self.windows.iter_mut().enumerate() {
            window.is_focused = idx == target_idx;
        }
        Ok(())
    }

    fn move_direction(&mut self, _direction: Direction) -> Result<()> {
        self.move_calls += 1;
        Ok(())
    }

    fn move_column(&mut self, _direction: Direction) -> Result<()> {
        Ok(())
    }

    fn consume_into_column_and_move(
        &mut self,
        _direction: Direction,
        _original_tile_index: usize,
    ) -> Result<()> {
        Ok(())
    }

    fn resize_with_intent(&mut self, _intent: ResizeIntent) -> Result<()> {
        Ok(())
    }

    fn spawn(&mut self, _command: Vec<String>) -> Result<()> {
        Ok(())
    }

    fn focus_window_by_id(&mut self, id: u64) -> Result<()> {
        let mut matched = false;
        for window in &mut self.windows {
            if window.id == id {
                window.is_focused = true;
                matched = true;
            } else {
                window.is_focused = false;
            }
        }
        if !matched {
            return Err(anyhow!("window id {id} not found"));
        }
        Ok(())
    }
}

#[test]
fn nvim_to_wezterm_cross_domain_move_uses_transfer_pipeline() {
    let mut orchestrator = Orchestrator::default();
    let nvim_counters = DomainCounters::default();
    let wezterm_counters = DomainCounters::default();
    orchestrator.register_domain(Box::new(FakeDomain::new(
        EDITOR_DOMAIN_ID,
        "nvim",
        vec![TypeId::of::<NvimPanePayload>()],
        Some(Box::new(NvimPanePayload)),
        nvim_counters.clone(),
    )));
    orchestrator.register_domain(Box::new(FakeDomain::new(
        TERMINAL_DOMAIN_ID,
        "wezterm",
        vec![TypeId::of::<NvimPanePayload>()],
        None,
        wezterm_counters.clone(),
    )));

    let mut wm = FakeWindowManager {
        windows: vec![
            WindowRecord {
                id: 1001,
                app_id: Some("emacs".to_string()),
                title: Some("nvim-like-source".to_string()),
                pid: None,
                is_focused: true,
                original_tile_index: 1,
            },
            WindowRecord {
                id: 2002,
                app_id: Some("org.wezfurlong.wezterm".to_string()),
                title: Some("wezterm-target".to_string()),
                pid: None,
                is_focused: false,
                original_tile_index: 2,
            },
        ],
        move_calls: 0,
    };

    orchestrator
        .execute(
            &mut wm,
            ActionRequest {
                kind: ActionKind::Move,
                direction: Direction::East,
            },
        )
        .expect("cross-domain move should execute");

    assert_eq!(wm.move_calls, 0);
    assert_eq!(nvim_counters.tear_off_calls.load(Ordering::Relaxed), 1);
    assert_eq!(wezterm_counters.merge_calls.load(Ordering::Relaxed), 1);
}

#[test]
fn wezterm_to_wm_cross_domain_move_falls_back_when_transfer_is_unsupported() {
    let mut orchestrator = Orchestrator::default();
    let wezterm_counters = DomainCounters::default();
    let wm_counters = DomainCounters::default();
    orchestrator.register_domain(Box::new(FakeDomain::new(
        TERMINAL_DOMAIN_ID,
        "wezterm",
        vec![TypeId::of::<WeztermPanePayload>()],
        Some(Box::new(WeztermPanePayload)),
        wezterm_counters.clone(),
    )));
    orchestrator.register_domain(Box::new(FakeDomain::new(
        WM_DOMAIN_ID,
        "wm",
        vec![],
        None,
        wm_counters.clone(),
    )));

    let mut wm = FakeWindowManager {
        windows: vec![
            WindowRecord {
                id: 3003,
                app_id: Some("org.wezfurlong.wezterm".to_string()),
                title: Some("wezterm-source".to_string()),
                pid: None,
                is_focused: true,
                original_tile_index: 1,
            },
            WindowRecord {
                id: 4004,
                app_id: Some("firefox".to_string()),
                title: Some("wm-target".to_string()),
                pid: None,
                is_focused: false,
                original_tile_index: 2,
            },
        ],
        move_calls: 0,
    };

    orchestrator
        .execute(
            &mut wm,
            ActionRequest {
                kind: ActionKind::Move,
                direction: Direction::East,
            },
        )
        .expect("cross-domain move should still succeed via wm fallback");

    assert_eq!(wm.move_calls, 1);
    assert_eq!(wezterm_counters.tear_off_calls.load(Ordering::Relaxed), 1);
    assert_eq!(wm_counters.merge_calls.load(Ordering::Relaxed), 0);
}
