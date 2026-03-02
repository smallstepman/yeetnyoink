use std::any::TypeId;

use anyhow::Result;

use crate::engine::domain::ErasedDomain;
use crate::engine::pane_state::{PaneState, PayloadRegistry, TransferError};
use crate::engine::topology::Cardinal;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferOutcome {
    Applied { merged_native_id: Vec<u8> },
    Fallback { reason: String },
}

pub struct TransferPipeline<'a> {
    registry: &'a PayloadRegistry,
}

impl<'a> TransferPipeline<'a> {
    pub fn new(registry: &'a PayloadRegistry) -> Self {
        Self { registry }
    }

    pub fn transfer_between(
        &self,
        source: &mut dyn ErasedDomain,
        source_native_id: &[u8],
        target: &mut dyn ErasedDomain,
        target_native_id: &[u8],
        dir: Cardinal,
    ) -> Result<TransferOutcome> {
        let payload = source.tear_off(source_native_id)?;
        let source_type = payload.as_ref().type_id();
        let supported = target.supported_payload_types();
        let converted = match self.convert_payload_for_target(
            payload,
            source_type,
            target.domain_name(),
            &supported,
        ) {
            Ok(value) => value,
            Err(reason) => return Ok(TransferOutcome::Fallback { reason }),
        };

        let merged_native_id = target.merge_in(target_native_id, dir, converted)?;
        // Mandatory resync hooks after topology-changing operations.
        let _ = source.fetch_snapshot()?;
        let _ = target.fetch_snapshot()?;

        Ok(TransferOutcome::Applied { merged_native_id })
    }

    pub fn transfer_within(
        &self,
        domain: &mut dyn ErasedDomain,
        source_native_id: &[u8],
        target_native_id: &[u8],
        dir: Cardinal,
    ) -> Result<TransferOutcome> {
        let payload = domain.tear_off(source_native_id)?;
        let source_type = payload.as_ref().type_id();
        let supported = domain.supported_payload_types();
        let converted = match self.convert_payload_for_target(
            payload,
            source_type,
            domain.domain_name(),
            &supported,
        ) {
            Ok(value) => value,
            Err(reason) => return Ok(TransferOutcome::Fallback { reason }),
        };

        let merged_native_id = domain.merge_in(target_native_id, dir, converted)?;
        let _ = domain.fetch_snapshot()?;

        Ok(TransferOutcome::Applied { merged_native_id })
    }

    fn convert_payload_for_target(
        &self,
        payload: Box<dyn PaneState>,
        source_type: TypeId,
        target_domain_name: &str,
        supported: &[TypeId],
    ) -> std::result::Result<Box<dyn PaneState>, String> {
        let Some(target_type) = pick_target_type(self.registry, source_type, supported) else {
            return Err(format!(
                "no compatible payload type from {:?} for target domain '{}'",
                source_type, target_domain_name
            ));
        };

        match self.registry.convert(payload, target_type) {
            Ok(value) => Ok(value),
            Err(TransferError::MissingConverter { .. }) => Err(format!(
                "missing payload converter from {:?} to {:?}",
                source_type, target_type
            )),
            Err(err) => Err(err.to_string()),
        }
    }
}

fn pick_target_type(
    registry: &PayloadRegistry,
    source_type: TypeId,
    supported: &[TypeId],
) -> Option<TypeId> {
    supported
        .iter()
        .copied()
        .find(|candidate| registry.can_convert(source_type, *candidate))
}

#[cfg(test)]
mod tests {
    use std::any::TypeId;

    use anyhow::{anyhow, Result};

    use crate::engine::domain::{DomainLeafSnapshot, DomainSnapshot, ErasedDomain};
    use crate::engine::pane_state::{PaneState, PayloadRegistry};
    use crate::engine::topology::Rect;

    use super::{TransferOutcome, TransferPipeline};

    #[derive(Debug)]
    struct SourcePayload;

    #[derive(Debug)]
    struct TargetPayload;

    struct FakeDomain {
        id: u64,
        name: &'static str,
        supported_payloads: Vec<TypeId>,
        tear_payload: Option<Box<dyn PaneState>>,
        merged_payload_type: Option<TypeId>,
        snapshot_reads: usize,
    }

    impl FakeDomain {
        fn new(id: u64, name: &'static str, supported_payloads: Vec<TypeId>) -> Self {
            Self {
                id,
                name,
                supported_payloads,
                tear_payload: None,
                merged_payload_type: None,
                snapshot_reads: 0,
            }
        }
    }

    impl ErasedDomain for FakeDomain {
        fn domain_id(&self) -> u64 {
            self.id
        }

        fn domain_name(&self) -> &'static str {
            self.name
        }

        fn rect(&self) -> Rect {
            Rect {
                x: 0,
                y: 0,
                w: 1000,
                h: 1000,
            }
        }

        fn fetch_snapshot(&mut self) -> Result<DomainSnapshot> {
            self.snapshot_reads += 1;
            Ok(DomainSnapshot {
                domain_id: self.id,
                rect: self.rect(),
                leaves: vec![DomainLeafSnapshot {
                    id: 1,
                    native_id: vec![1],
                    rect: self.rect(),
                    focused: true,
                }],
            })
        }

        fn supported_payload_types(&self) -> Vec<TypeId> {
            self.supported_payloads.clone()
        }

        fn tear_off(&mut self, _native_id: &[u8]) -> Result<Box<dyn PaneState>> {
            self.tear_payload
                .take()
                .ok_or_else(|| anyhow!("no payload available"))
        }

        fn merge_in(
            &mut self,
            _target_native_id: &[u8],
            _dir: crate::engine::topology::Cardinal,
            payload: Box<dyn PaneState>,
        ) -> Result<Vec<u8>> {
            self.merged_payload_type = Some(payload.as_ref().type_id());
            Ok(vec![9, 9, 9])
        }
    }

    #[test]
    fn transfer_pipeline_converts_and_merges_payload() {
        let mut registry = PayloadRegistry::default();
        registry.register(|_payload: SourcePayload| TargetPayload);
        let pipeline = TransferPipeline::new(&registry);

        let mut source = FakeDomain::new(1, "source", vec![TypeId::of::<SourcePayload>()]);
        source.tear_payload = Some(Box::new(SourcePayload));
        let mut target = FakeDomain::new(2, "target", vec![TypeId::of::<TargetPayload>()]);

        let outcome = pipeline
            .transfer_between(
                &mut source,
                &[1],
                &mut target,
                &[2],
                crate::engine::topology::Cardinal::East,
            )
            .expect("transfer should succeed");

        assert!(matches!(outcome, TransferOutcome::Applied { .. }));
        assert_eq!(
            target.merged_payload_type,
            Some(TypeId::of::<TargetPayload>())
        );
        assert!(source.snapshot_reads > 0);
        assert!(target.snapshot_reads > 0);
    }

    #[test]
    fn transfer_pipeline_falls_back_without_converter() {
        let registry = PayloadRegistry::default();
        let pipeline = TransferPipeline::new(&registry);

        let mut source = FakeDomain::new(1, "source", vec![TypeId::of::<SourcePayload>()]);
        source.tear_payload = Some(Box::new(SourcePayload));
        let mut target = FakeDomain::new(2, "target", vec![TypeId::of::<TargetPayload>()]);

        let outcome = pipeline
            .transfer_between(
                &mut source,
                &[1],
                &mut target,
                &[2],
                crate::engine::topology::Cardinal::East,
            )
            .expect("fallback path should succeed");

        assert!(matches!(outcome, TransferOutcome::Fallback { .. }));
        assert_eq!(target.merged_payload_type, None);
    }

    #[test]
    fn transfer_pipeline_supports_within_domain_merge() {
        let mut registry = PayloadRegistry::default();
        registry.register(|_payload: SourcePayload| TargetPayload);
        let pipeline = TransferPipeline::new(&registry);

        let mut domain = FakeDomain::new(1, "terminal", vec![TypeId::of::<TargetPayload>()]);
        domain.tear_payload = Some(Box::new(SourcePayload));

        let outcome = pipeline
            .transfer_within(
                &mut domain,
                &[1],
                &[2],
                crate::engine::topology::Cardinal::West,
            )
            .expect("within-domain transfer should succeed");

        assert!(matches!(outcome, TransferOutcome::Applied { .. }));
        assert_eq!(
            domain.merged_payload_type,
            Some(TypeId::of::<TargetPayload>())
        );
        assert!(domain.snapshot_reads > 0);
    }
}
