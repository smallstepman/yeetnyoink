// Re-export shim — contents have been migrated to engine::contracts.
// Preserved for backward compatibility: adapters import types from engine::contract.
pub use crate::engine::contracts::*;

use crate::engine::runtime::ProcessId;
use crate::engine::topology::DomainId;

// ChainResolver and its blanket impl remain here temporarily until Task 7.

pub trait ChainResolver {
    fn resolve_chain(&self, app_id: &str, pid: u32, title: &str) -> Vec<Box<dyn AppAdapter>>;
    fn default_domain_adapters(&self) -> Vec<Box<dyn AppAdapter>>;
    fn domain_id_for_window(
        &self,
        app_id: Option<&str>,
        pid: Option<ProcessId>,
        title: Option<&str>,
    ) -> DomainId;
}

impl<T> ChainResolver for T
where
    T: TopologyHandler + ?Sized,
{
    fn resolve_chain(&self, app_id: &str, pid: u32, title: &str) -> Vec<Box<dyn AppAdapter>> {
        crate::engine::chain_resolver::resolve_app_chain(app_id, pid, title)
    }

    fn default_domain_adapters(&self) -> Vec<Box<dyn AppAdapter>> {
        crate::engine::chain_resolver::default_app_domain_adapters()
    }

    fn domain_id_for_window(
        &self,
        app_id: Option<&str>,
        pid: Option<ProcessId>,
        title: Option<&str>,
    ) -> DomainId {
        crate::engine::chain_resolver::resolve_window_domain_id(app_id, pid, title)
    }
}

