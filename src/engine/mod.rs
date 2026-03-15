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
// Backward-compatible module aliases — replaces the deleted shim files so
// adapter and command files that import engine::contract::X / engine::domain::X
// / engine::window_manager::X / engine::orchestrator::X continue to compile
// without changes.
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

/// Backward-compat alias for `engine::resolution` chain functions.
pub(crate) mod chain_resolver {
    pub(crate) use crate::engine::resolution::{
        default_app_domain_adapters, resolve_app_chain, resolve_window_domain_id,
    };
}
