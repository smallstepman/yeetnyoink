pub(crate) mod context;
pub(crate) mod focus;
pub(crate) mod merge;
pub(crate) mod movement;
pub(crate) mod probe;
pub(crate) mod resize;
pub(crate) mod tearout;
// Re-exported for use throughout the actions module and beyond; walk_chain
// is unused until later tasks consume it.  merge::* and tearout::* are used
// by orchestrator test helpers (cleanup_merged_source_window, etc.) which are
// only compiled in test builds.
#[allow(unused_imports)]
pub(crate) use context::{AppContext, walk_chain};
#[allow(unused_imports)]
pub(crate) use merge::*;
pub(crate) use probe::*;
#[allow(unused_imports)]
pub(crate) use tearout::*;

pub(crate) mod orchestrator;
pub use orchestrator::{ActionKind, ActionRequest, RoutingDecision, RoutingError, Orchestrator};
