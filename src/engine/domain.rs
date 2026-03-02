use std::any::TypeId;

use anyhow::Result as AnyResult;

use crate::engine::pane_state::PaneState;
use crate::engine::topology::{Cardinal, DomainId, LeafId, Rect};

mod sealed {
    use std::marker::PhantomData;

    #[must_use = "topology is stale until a fresh layout snapshot is fetched"]
    pub struct TopologyChanged(pub(super) PhantomData<()>);

    impl TopologyChanged {
        pub(super) fn new() -> Self {
            Self(PhantomData)
        }
    }
}

pub use sealed::TopologyChanged;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainLeafSnapshot {
    pub id: LeafId,
    pub native_id: Vec<u8>,
    pub rect: Rect,
    pub focused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainSnapshot {
    pub domain_id: DomainId,
    pub rect: Rect,
    pub leaves: Vec<DomainLeafSnapshot>,
}

pub trait TopologyProvider {
    type NativeId: Clone + Send + 'static;
    type Error;

    fn domain_name(&self) -> &'static str;
    fn rect(&self) -> Rect;
    fn fetch_layout(&mut self) -> Result<(), Self::Error>;
}

pub trait TopologyModifierImpl: TopologyProvider {
    fn focus_impl(&mut self, id: &Self::NativeId) -> Result<(), Self::Error>;
    fn move_impl(&mut self, id: &Self::NativeId, dir: Cardinal) -> Result<(), Self::Error>;
    fn tear_off_impl(&mut self, id: &Self::NativeId) -> Result<Box<dyn PaneState>, Self::Error>;
    fn merge_in_impl(
        &mut self,
        target: &Self::NativeId,
        dir: Cardinal,
        payload: Box<dyn PaneState>,
    ) -> Result<Self::NativeId, Self::Error>;
}

pub trait TopologyModifier: TopologyModifierImpl {
    fn focus(&mut self, id: &Self::NativeId) -> Result<TopologyChanged, Self::Error> {
        self.focus_impl(id)?;
        Ok(sealed::TopologyChanged::new())
    }

    fn move_pane(
        &mut self,
        id: &Self::NativeId,
        dir: Cardinal,
    ) -> Result<TopologyChanged, Self::Error> {
        self.move_impl(id, dir)?;
        Ok(sealed::TopologyChanged::new())
    }

    fn tear_off(
        &mut self,
        id: &Self::NativeId,
    ) -> Result<(Box<dyn PaneState>, TopologyChanged), Self::Error> {
        let payload = self.tear_off_impl(id)?;
        Ok((payload, sealed::TopologyChanged::new()))
    }

    fn merge_in(
        &mut self,
        target: &Self::NativeId,
        dir: Cardinal,
        payload: Box<dyn PaneState>,
    ) -> Result<(Self::NativeId, TopologyChanged), Self::Error> {
        let id = self.merge_in_impl(target, dir, payload)?;
        Ok((id, sealed::TopologyChanged::new()))
    }
}

impl<T> TopologyModifier for T where T: TopologyModifierImpl {}

pub trait TilingDomain: TopologyModifier {
    fn supported_payload_types(&self) -> &'static [TypeId];
}

/// Object-safe runtime adapter for cross-domain orchestration.
pub trait ErasedDomain: Send {
    fn domain_id(&self) -> DomainId;
    fn domain_name(&self) -> &'static str;
    fn rect(&self) -> Rect;
    fn fetch_snapshot(&mut self) -> AnyResult<DomainSnapshot>;
    fn supported_payload_types(&self) -> Vec<TypeId>;
    fn tear_off(&mut self, native_id: &[u8]) -> AnyResult<Box<dyn PaneState>>;
    fn merge_in(
        &mut self,
        target_native_id: &[u8],
        dir: Cardinal,
        payload: Box<dyn PaneState>,
    ) -> AnyResult<Vec<u8>>;
}
