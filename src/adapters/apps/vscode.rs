use anyhow::Result;

use crate::engine::contracts::{
    unsupported_operation, AdapterCapabilities, AppKind, DeepApp, MoveDecision, TearResult,
    TopologyModifier, TopologyProvider,
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

    fn can_focus(&self, _dir: Direction, _pid: u32) -> Result<bool> {
        Ok(false)
    }

    fn focus(&self, _dir: Direction, _pid: u32) -> Result<()> {
        Err(unsupported_operation(self.adapter_name(), "focus"))
    }

    fn move_decision(&self, _dir: Direction, _pid: u32) -> Result<MoveDecision> {
        Ok(MoveDecision::Passthrough)
    }

    fn move_internal(&self, _dir: Direction, _pid: u32) -> Result<()> {
        Err(unsupported_operation(self.adapter_name(), "move_internal"))
    }

    fn move_out(&self, _dir: Direction, _pid: u32) -> Result<TearResult> {
        Err(unsupported_operation(self.adapter_name(), "move_out"))
    }
}

impl TopologyProvider for Vscode {}
impl TopologyModifier for Vscode {}

#[cfg(test)]
mod tests {
    use super::Vscode;
    use crate::engine::contracts::DeepApp;

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
