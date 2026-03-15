pub(crate) mod actions;
pub(crate) mod contracts;
pub(crate) mod resolution;
pub mod runtime;
pub mod topology;
pub(crate) mod transfer;
pub(crate) mod wm;

// Flat re-exports — consumers can use engine::X directly
pub use contracts::*;
pub use transfer::*;
pub use wm::*;
pub use actions::{ActionKind, ActionRequest, RoutingDecision, RoutingError, Orchestrator};

// ---------------------------------------------------------------------------
// Path-compatibility shims: these inline modules preserve existing import paths
// across the codebase (engine::contract::X, engine::domain::X, etc.) while the
// canonical items live in their new layer modules.
// ---------------------------------------------------------------------------

/// Backward-compat alias for `engine::contracts`.
pub mod contract {
    pub use crate::engine::contracts::*;
}

/// Backward-compat alias for `engine::transfer`.
pub mod domain {
    pub use crate::engine::transfer::*;
}

/// Backward-compat alias for `engine::wm`.
pub mod window_manager {
    pub use crate::engine::wm::*;
}

/// Backward-compat alias for `engine::actions` orchestrator types.
pub mod orchestrator {
    pub use crate::engine::actions::{
        ActionKind, ActionRequest, RoutingDecision, RoutingError, Orchestrator,
    };
}
