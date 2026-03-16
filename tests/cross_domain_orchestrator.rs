use std::any::TypeId;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};

use anyhow::{anyhow, Result};

use yeet_and_yoink::engine::topology::Direction;
use yeet_and_yoink::engine::topology::Rect;
use yeet_and_yoink::engine::PaneState;
use yeet_and_yoink::engine::{ActionKind, ActionRequest, Orchestrator};
use yeet_and_yoink::engine::{
    ConfiguredWindowManager, FocusedWindowRecord, ResizeIntent, WindowManagerCapabilities,
    WindowManagerFeatures, WindowManagerSession, WindowRecord,
};
use yeet_and_yoink::engine::{DomainLeafSnapshot, DomainSnapshot, ErasedDomain};
use yeet_and_yoink::engine::{EDITOR_DOMAIN_ID, TERMINAL_DOMAIN_ID, WM_DOMAIN_ID};

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

struct FakeWindowManagerState {
    windows: Vec<WindowRecord>,
    move_calls: usize,
}

struct FakeWindowManager {
    state: Arc<Mutex<FakeWindowManagerState>>,
}

impl FakeWindowManager {
    fn new(state: Arc<Mutex<FakeWindowManagerState>>) -> Self {
        Self { state }
    }
}

fn configured_fake_wm(
    state: FakeWindowManagerState,
) -> (ConfiguredWindowManager, Arc<Mutex<FakeWindowManagerState>>) {
    let state = Arc::new(Mutex::new(state));
    (
        ConfiguredWindowManager::new(
            Box::new(FakeWindowManager::new(state.clone())),
            WindowManagerFeatures::default(),
        ),
        state,
    )
}

fn env_guard() -> std::sync::MutexGuard<'static, ()> {
    static ENV_GUARD: OnceLock<Mutex<()>> = OnceLock::new();
    ENV_GUARD
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("env guard should lock")
}

fn load_config(path: &std::path::Path) -> yeet_and_yoink::config::Config {
    let old = yeet_and_yoink::config::snapshot();
    yeet_and_yoink::config::prepare_with_path(Some(path)).expect("config should load");
    old
}

fn restore_config(old: yeet_and_yoink::config::Config) {
    yeet_and_yoink::config::install(old);
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "yeet-and-yoink-cross-domain-{prefix}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be monotonic")
            .as_nanos()
    ))
}

impl WindowManagerSession for FakeWindowManager {
    fn adapter_name(&self) -> &'static str {
        "fake"
    }

    fn capabilities(&self) -> WindowManagerCapabilities {
        WindowManagerCapabilities::none()
    }

    fn focused_window(&mut self) -> Result<FocusedWindowRecord> {
        let state = self
            .state
            .lock()
            .expect("fake wm state mutex should not be poisoned");
        let focused = state
            .windows
            .iter()
            .find(|window| window.is_focused)
            .ok_or_else(|| anyhow!("no focused window"))?;
        Ok(FocusedWindowRecord {
            id: focused.id,
            app_id: focused.app_id.clone(),
            title: focused.title.clone(),
            pid: focused.pid,
            original_tile_index: focused.original_tile_index,
        })
    }

    fn windows(&mut self) -> Result<Vec<WindowRecord>> {
        Ok(self
            .state
            .lock()
            .expect("fake wm state mutex should not be poisoned")
            .windows
            .clone())
    }

    fn focus_direction(&mut self, _direction: Direction) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .expect("fake wm state mutex should not be poisoned");
        if state.windows.len() < 2 {
            return Ok(());
        }
        let focused_idx = state
            .windows
            .iter()
            .position(|window| window.is_focused)
            .ok_or_else(|| anyhow!("no focused window"))?;
        let target_idx = if focused_idx + 1 < state.windows.len() {
            focused_idx + 1
        } else {
            focused_idx.saturating_sub(1)
        };
        for (idx, window) in state.windows.iter_mut().enumerate() {
            window.is_focused = idx == target_idx;
        }
        Ok(())
    }

    fn move_direction(&mut self, _direction: Direction) -> Result<()> {
        self.state
            .lock()
            .expect("fake wm state mutex should not be poisoned")
            .move_calls += 1;
        Ok(())
    }

    fn resize_with_intent(&mut self, _intent: ResizeIntent) -> Result<()> {
        Ok(())
    }

    fn spawn(&mut self, _command: Vec<String>) -> Result<()> {
        Ok(())
    }

    fn focus_window_by_id(&mut self, id: u64) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .expect("fake wm state mutex should not be poisoned");
        let mut matched = false;
        for window in &mut state.windows {
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

    fn close_window_by_id(&mut self, id: u64) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .expect("fake wm state mutex should not be poisoned");
        let original_len = state.windows.len();
        state.windows.retain(|window| window.id != id);
        if state.windows.len() == original_len {
            return Err(anyhow!("window id {id} not found"));
        }
        Ok(())
    }
}

#[test]
fn nvim_to_wezterm_cross_domain_move_uses_transfer_pipeline() {
    let _guard = env_guard();
    let root = unique_temp_dir("nvim-wezterm");
    let config_dir = root.join("yeet-and-yoink");
    fs::create_dir_all(&config_dir).expect("config dir should be created");
    let config_path = config_dir.join("config.toml");
    fs::write(
        &config_path,
        r#"
[app.editor.emacs]
enabled = true

[app.terminal.wezterm]
enabled = true
"#,
    )
    .expect("config file should be writable");
    let old_config = load_config(&config_path);

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

    let (mut wm, state) = configured_fake_wm(FakeWindowManagerState {
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
    });

    orchestrator
        .execute(
            &mut wm,
            ActionRequest {
                kind: ActionKind::Move,
                direction: Direction::East,
            },
        )
        .expect("cross-domain move should execute");

    assert_eq!(
        state
            .lock()
            .expect("fake wm state mutex should not be poisoned")
            .move_calls,
        0
    );
    assert_eq!(nvim_counters.tear_off_calls.load(Ordering::Relaxed), 1);
    assert_eq!(wezterm_counters.merge_calls.load(Ordering::Relaxed), 1);

    restore_config(old_config);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn wezterm_to_wm_cross_domain_move_falls_back_when_transfer_is_unsupported() {
    let _guard = env_guard();
    let root = unique_temp_dir("wezterm-wm");
    let config_dir = root.join("yeet-and-yoink");
    fs::create_dir_all(&config_dir).expect("config dir should be created");
    let config_path = config_dir.join("config.toml");
    fs::write(
        &config_path,
        r#"
[app.terminal.wezterm]
enabled = true
"#,
    )
    .expect("config file should be writable");
    let old_config = load_config(&config_path);

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

    let (mut wm, state) = configured_fake_wm(FakeWindowManagerState {
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
    });

    orchestrator
        .execute(
            &mut wm,
            ActionRequest {
                kind: ActionKind::Move,
                direction: Direction::East,
            },
        )
        .expect("cross-domain move should still succeed via wm fallback");

    assert_eq!(
        state
            .lock()
            .expect("fake wm state mutex should not be poisoned")
            .move_calls,
        1
    );
    assert_eq!(wezterm_counters.tear_off_calls.load(Ordering::Relaxed), 1);
    assert_eq!(wm_counters.merge_calls.load(Ordering::Relaxed), 0);

    restore_config(old_config);
    let _ = fs::remove_dir_all(root);
}
