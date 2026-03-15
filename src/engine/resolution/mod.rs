pub(crate) mod catalog;
pub(crate) mod chain;
pub(crate) mod command;
pub(crate) mod domain;
pub(crate) mod policy;

// Re-exports consumed by engine::chain_resolver backward-compat alias
pub(crate) use chain::{
    default_app_domain_adapters,
    resolve_app_chain,
    resolve_window_domain_id,
};
