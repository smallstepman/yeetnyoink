use crate::adapters::apps::AppKind;
use crate::engine::transfer::{EDITOR_DOMAIN_ID, TERMINAL_DOMAIN_ID, WM_DOMAIN_ID};
use crate::engine::topology::DomainId;

/// Maps an AppKind to its canonical domain ID.
/// Canonical copy — previously duplicated in chain_resolver.rs and domain.rs.
pub(crate) fn domain_id_for_app_kind(kind: AppKind) -> DomainId {
    match kind {
        AppKind::Terminal => TERMINAL_DOMAIN_ID,
        AppKind::Editor => EDITOR_DOMAIN_ID,
        AppKind::Browser => WM_DOMAIN_ID,
    }
}
