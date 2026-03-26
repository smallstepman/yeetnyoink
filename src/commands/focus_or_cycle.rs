use anyhow::{Result, bail};
use clap::Args;

use crate::engine::wm::{ConfiguredWindowManager, WindowCycleRequest, connect_selected};

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

impl FocusOrCycleArgs {
    fn try_into_request(self) -> Result<WindowCycleRequest> {
        if self.app_id.is_none() && self.title.is_none() {
            bail!("focus-or-cycle requires --app-id and/or --title");
        }

        if self.new && self.spawn.is_none() {
            bail!("--new requires --spawn '<command>'");
        }

        Ok(WindowCycleRequest {
            app_id: self.app_id,
            title: self.title,
            spawn: self.spawn,
            new: self.new,
            summon: self.summon,
        })
    }
}

pub fn run(args: FocusOrCycleArgs) -> Result<()> {
    let request = args.try_into_request()?;
    let mut wm = connect_selected()?;
    run_with_window_manager(request, &mut wm)
}

fn run_with_window_manager(
    request: WindowCycleRequest,
    wm: &mut ConfiguredWindowManager,
) -> Result<()> {
    let adapter_name = wm.adapter_name();
    let provider = wm.window_cycle_mut().ok_or_else(|| {
        anyhow::anyhow!("window manager '{adapter_name}' does not support focus-or-cycle")
    })?;
    provider.focus_or_cycle(&request)
}

#[cfg(test)]
mod tests {
    use super::{FocusOrCycleArgs, run_with_window_manager};
    use crate::engine::topology::Direction;
    use crate::engine::{
        ConfiguredWindowManager, FocusedWindowRecord, ResizeIntent, WindowCycleProvider,
        WindowCycleRequest, WindowManagerCapabilities, WindowManagerFeatures, WindowManagerSession,
        WindowRecord,
    };
    use anyhow::Result;
    use std::sync::{Arc, Mutex};

    #[test]
    fn focus_or_cycle_dispatches_through_window_cycle_provider() {
        let mut wm = fake_wm_with_cycle_provider();
        run_with_window_manager(sample_request(), &mut wm.wm).unwrap();
        assert_eq!(wm.take_cycle_calls(), vec![sample_request()]);
    }

    #[test]
    fn focus_or_cycle_returns_clear_error_when_capability_is_missing() {
        let mut wm = fake_wm_without_cycle_provider();
        let err = run_with_window_manager(sample_request(), &mut wm).unwrap_err();
        assert!(err.to_string().contains("does not support focus-or-cycle"));
    }

    fn sample_request() -> WindowCycleRequest {
        FocusOrCycleArgs {
            app_id: Some("org.example.App".to_string()),
            title: Some("Project".to_string()),
            spawn: Some("app-launch".to_string()),
            new: false,
            summon: false,
        }
        .try_into_request()
        .expect("sample request should be valid")
    }

    fn fake_wm_with_cycle_provider() -> TestConfiguredWindowManager {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut features = WindowManagerFeatures::default();
        features.window_cycle = Some(Box::new(FakeCycleProvider::new(calls.clone())));
        TestConfiguredWindowManager::new(
            ConfiguredWindowManager::new(Box::new(FakeSession), features),
            calls,
        )
    }

    fn fake_wm_without_cycle_provider() -> ConfiguredWindowManager {
        ConfiguredWindowManager::new(Box::new(FakeSession), WindowManagerFeatures::default())
    }

    struct TestConfiguredWindowManager {
        wm: ConfiguredWindowManager,
        cycle_calls: Arc<Mutex<Vec<WindowCycleRequest>>>,
    }

    impl TestConfiguredWindowManager {
        fn new(
            wm: ConfiguredWindowManager,
            cycle_calls: Arc<Mutex<Vec<WindowCycleRequest>>>,
        ) -> Self {
            Self { wm, cycle_calls }
        }

        fn take_cycle_calls(&mut self) -> Vec<WindowCycleRequest> {
            let mut cycle_calls = self
                .cycle_calls
                .lock()
                .expect("cycle calls mutex should not be poisoned");
            std::mem::take(&mut *cycle_calls)
        }
    }

    struct FakeSession;

    impl WindowManagerSession for FakeSession {
        fn adapter_name(&self) -> &'static str {
            "fake"
        }

        fn capabilities(&self) -> WindowManagerCapabilities {
            WindowManagerCapabilities::none()
        }

        fn focused_window(&mut self) -> Result<FocusedWindowRecord> {
            Ok(FocusedWindowRecord {
                id: 1,
                app_id: Some("fake-app".to_string()),
                title: Some("fake-title".to_string()),
                pid: None,
                original_tile_index: 1,
            })
        }

        fn windows(&mut self) -> Result<Vec<WindowRecord>> {
            Ok(Vec::new())
        }

        fn focus_direction(&mut self, _direction: Direction) -> Result<()> {
            Ok(())
        }

        fn move_direction(&mut self, _direction: Direction) -> Result<()> {
            Ok(())
        }

        fn resize_with_intent(&mut self, _intent: ResizeIntent) -> Result<()> {
            Ok(())
        }

        fn spawn(&mut self, _command: Vec<String>) -> Result<()> {
            Ok(())
        }

        fn focus_window_by_id(&mut self, _id: u64) -> Result<()> {
            Ok(())
        }

        fn close_window_by_id(&mut self, _id: u64) -> Result<()> {
            Ok(())
        }
    }

    struct FakeCycleProvider {
        cycle_calls: Arc<Mutex<Vec<WindowCycleRequest>>>,
    }

    impl FakeCycleProvider {
        fn new(cycle_calls: Arc<Mutex<Vec<WindowCycleRequest>>>) -> Self {
            Self { cycle_calls }
        }
    }

    impl WindowCycleProvider for FakeCycleProvider {
        fn focus_or_cycle(&mut self, request: &WindowCycleRequest) -> Result<()> {
            self.cycle_calls
                .lock()
                .expect("cycle calls mutex should not be poisoned")
                .push(request.clone());
            Ok(())
        }
    }
}
