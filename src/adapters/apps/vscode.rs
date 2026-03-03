use anyhow::Result;

use crate::engine::contract::{
    unsupported_operation, AdapterCapabilities, AppKind, DeepApp, MoveDecision, TearResult,
    TopologyHandler,
};
use crate::engine::topology::Direction;

/// VS Code / Code OSS — stub until extension IPC is available.
pub struct Vscode;

impl DeepApp for Vscode {
    fn adapter_name(&self) -> &'static str {
        "vscode"
    }

    fn kind(&self) -> AppKind {
        AppKind::Editor
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            probe: true,
            focus: false,
            move_internal: false,
            resize_internal: false,
            rearrange: false,
            tear_out: false,
            merge: false,
        }
    }
}

impl TopologyHandler for Vscode {
    fn can_focus(&self, _dir: Direction, _pid: u32) -> Result<bool> {
        Ok(false)
    }

    fn move_decision(&self, _dir: Direction, _pid: u32) -> Result<MoveDecision> {
        Ok(MoveDecision::Passthrough)
    }

    fn focus(&self, _dir: Direction, _pid: u32) -> Result<()> {
        Err(unsupported_operation(self.adapter_name(), "focus"))
    }

    fn move_internal(&self, _dir: Direction, _pid: u32) -> Result<()> {
        Err(unsupported_operation(self.adapter_name(), "move_internal"))
    }

    fn move_out(&self, _dir: Direction, _pid: u32) -> Result<TearResult> {
        Err(unsupported_operation(self.adapter_name(), "move_out"))
    }
}

#[cfg(test)]
mod tests {
    use super::Vscode;
    use crate::engine::contract::DeepApp;

    #[test]
    fn declares_explicit_capability_contract() {
        let app = Vscode;
        let caps = DeepApp::capabilities(&app);
        assert!(caps.probe);
        assert!(!caps.focus);
        assert!(!caps.move_internal);
        assert!(!caps.resize_internal);
        assert!(!caps.rearrange);
        assert!(!caps.tear_out);
        assert!(!caps.merge);
    }
}
