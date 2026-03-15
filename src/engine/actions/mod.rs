pub(crate) mod context;
pub(crate) mod focus;
pub(crate) mod merge;
pub(crate) mod movement;
pub(crate) mod probe;
pub(crate) mod resize;
pub(crate) mod tearout;
// merge::* and tearout::* are used by orchestrator test helpers
// (cleanup_merged_source_window, etc.) which are only compiled in test builds.
#[allow(unused_imports)]
pub(crate) use merge::*;
pub(crate) use probe::*;
#[allow(unused_imports)]
pub(crate) use tearout::*;

pub(crate) mod orchestrator;
pub use orchestrator::{ActionKind, ActionRequest, RoutingDecision, RoutingError, Orchestrator};
