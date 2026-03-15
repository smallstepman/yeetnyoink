pub(crate) mod catalog;
pub(crate) mod chain;
pub(crate) mod command;
pub(crate) mod domain;
pub(crate) mod policy;

pub(crate) use policy::bind_app_policy;

// Re-export the public API previously available via engine::chain_resolver
pub(crate) use chain::{
    RuntimeChainResolver,
    default_app_domain_adapters,
    resolve_app_chain,
    resolve_window_domain_id,
    runtime_chain_resolver,
};
