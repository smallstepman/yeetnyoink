pub(crate) mod context;
pub(crate) mod focus;
pub(crate) mod merge;
pub(crate) mod movement;
pub(crate) mod probe;
pub(crate) mod resize;
pub(crate) mod tearout;
// Re-exported for use throughout the actions module and beyond; walk_chain
// is unused until later tasks consume it.
#[allow(unused_imports)]
pub(crate) use context::{AppContext, walk_chain};
pub(crate) use merge::*;
pub(crate) use probe::*;
pub(crate) use tearout::*;

pub(crate) mod orchestrator;
pub use orchestrator::{ActionKind, ActionRequest, RoutingDecision, RoutingError, Orchestrator};
