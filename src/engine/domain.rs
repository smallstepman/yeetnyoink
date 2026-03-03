use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};

use anyhow::Result as AnyResult;

use crate::engine::topology::Direction;
use crate::engine::topology::{DomainId, LeafId, Rect};

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

#[derive(Debug)]
pub enum TransferError {
    MissingConverter { from: TypeId, to: TypeId },
    DowncastFailed { expected: TypeId },
    ConversionFailed(String),
}

impl std::fmt::Display for TransferError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingConverter { from, to } => {
                write!(
                    f,
                    "no payload converter registered from {:?} to {:?}",
                    from, to
                )
            }
            Self::DowncastFailed { expected } => {
                write!(f, "failed to downcast payload to {:?}", expected)
            }
            Self::ConversionFailed(reason) => write!(f, "payload conversion failed: {reason}"),
        }
    }
}

impl std::error::Error for TransferError {}

pub trait PaneState: Any + Send {
    fn into_any(self: Box<Self>) -> Box<dyn Any + Send>;
}

impl<T> PaneState for T
where
    T: Any + Send,
{
    fn into_any(self: Box<Self>) -> Box<dyn Any + Send> {
        self
    }
}

type ConverterFn = Box<
    dyn Fn(Box<dyn PaneState>) -> Result<Box<dyn PaneState>, TransferError> + Send + Sync + 'static,
>;

#[derive(Default)]
pub struct PayloadRegistry {
    converters: HashMap<(TypeId, TypeId), ConverterFn>,
}

impl PayloadRegistry {
    pub fn register<From, To>(
        &mut self,
        converter: impl Fn(From) -> To + Send + Sync + 'static,
    ) -> &mut Self
    where
        From: PaneState + 'static,
        To: PaneState + 'static,
    {
        self.converters.insert(
            (TypeId::of::<From>(), TypeId::of::<To>()),
            Box::new(move |payload| {
                let any = payload.into_any();
                let source = any
                    .downcast::<From>()
                    .map_err(|_| TransferError::DowncastFailed {
                        expected: TypeId::of::<From>(),
                    })?;
                let converted = converter(*source);
                Ok(Box::new(converted))
            }),
        );
        self
    }

    pub fn convert(
        &self,
        payload: Box<dyn PaneState>,
        target_type: TypeId,
    ) -> Result<Box<dyn PaneState>, TransferError> {
        let source_type = payload.as_ref().type_id();
        if source_type == target_type {
            return Ok(payload);
        }
        let converter = self.converters.get(&(source_type, target_type)).ok_or(
            TransferError::MissingConverter {
                from: source_type,
                to: target_type,
            },
        )?;
        converter(payload)
    }

    pub fn can_convert(&self, from: TypeId, to: TypeId) -> bool {
        from == to || self.converters.contains_key(&(from, to))
    }
}

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
    fn move_impl(&mut self, id: &Self::NativeId, dir: Direction) -> Result<(), Self::Error>;
    fn tear_off_impl(&mut self, id: &Self::NativeId) -> Result<Box<dyn PaneState>, Self::Error>;
    fn merge_in_impl(
        &mut self,
        target: &Self::NativeId,
        dir: Direction,
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
        dir: Direction,
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
        dir: Direction,
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
        dir: Direction,
        payload: Box<dyn PaneState>,
    ) -> AnyResult<Vec<u8>>;
}

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
        dir: Direction,
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
        dir: Direction,
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

use std::sync::OnceLock;

use anyhow::{anyhow, Context, Result};

use crate::adapters::apps::editor_backend::EditorBackend;
use crate::adapters::apps::terminal_backend::TerminalBackend;
use crate::adapters::apps::{self, AppKind, DeepApp, MergePreparation};
use crate::adapters::window_managers::niri::NiriDomainPlugin;
use crate::adapters::window_managers::{FocusedWindowView, WindowManagerAdapter};
use crate::engine::runtime::ProcessId;

pub const WM_DOMAIN_ID: DomainId = 1;
pub const TERMINAL_DOMAIN_ID: DomainId = 2;
pub const EDITOR_DOMAIN_ID: DomainId = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeWindowRef {
    pub window_id: u64,
    pub pid: Option<ProcessId>,
}

pub fn encode_native_window_ref(window_id: u64, pid: Option<ProcessId>) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(12);
    encoded.extend_from_slice(&window_id.to_le_bytes());
    encoded.extend_from_slice(&pid.map(ProcessId::get).unwrap_or(0).to_le_bytes());
    encoded
}

pub fn decode_native_window_ref(bytes: &[u8]) -> Option<NativeWindowRef> {
    if bytes.len() != 12 {
        return None;
    }
    let mut window = [0_u8; 8];
    window.copy_from_slice(&bytes[..8]);
    let mut pid = [0_u8; 4];
    pid.copy_from_slice(&bytes[8..12]);
    Some(NativeWindowRef {
        window_id: u64::from_le_bytes(window),
        pid: ProcessId::new(u32::from_le_bytes(pid)),
    })
}

#[derive(Debug)]
struct AppMergePayload {
    source_pid: Option<ProcessId>,
    preparation: MergePreparation,
}

fn app_merge_payload_types() -> &'static [TypeId] {
    static TYPES: OnceLock<Vec<TypeId>> = OnceLock::new();
    TYPES
        .get_or_init(|| vec![TypeId::of::<AppMergePayload>()])
        .as_slice()
}

#[derive(Debug)]
struct UnsupportedDomainPlugin {
    domain_id: DomainId,
    name: String,
}

impl UnsupportedDomainPlugin {
    fn new(domain_id: DomainId, name: impl Into<String>) -> Self {
        Self {
            domain_id,
            name: name.into(),
        }
    }
}

impl ErasedDomain for UnsupportedDomainPlugin {
    fn domain_id(&self) -> DomainId {
        self.domain_id
    }

    fn domain_name(&self) -> &'static str {
        // The dynamic adapter name is only used in logs/debugging.
        "wm"
    }

    fn rect(&self) -> Rect {
        Rect {
            x: 0,
            y: 0,
            w: 10000,
            h: 10000,
        }
    }

    fn fetch_snapshot(&mut self) -> Result<DomainSnapshot> {
        Ok(DomainSnapshot {
            domain_id: self.domain_id,
            rect: self.rect(),
            leaves: vec![],
        })
    }

    fn supported_payload_types(&self) -> Vec<TypeId> {
        vec![]
    }

    fn tear_off(&mut self, _native_id: &[u8]) -> Result<Box<dyn PaneState>> {
        Err(anyhow!(
            "domain '{}' does not support tear-off payload transfer",
            self.name
        ))
    }

    fn merge_in(
        &mut self,
        _target_native_id: &[u8],
        _dir: Direction,
        _payload: Box<dyn PaneState>,
    ) -> Result<Vec<u8>> {
        Err(anyhow!(
            "domain '{}' does not support merge-in payload transfer",
            self.name
        ))
    }
}

pub struct AppDomainPlugin {
    domain_id: DomainId,
    adapter: Box<dyn DeepApp>,
}

impl AppDomainPlugin {
    pub fn new(domain_id: DomainId, adapter: Box<dyn DeepApp>) -> Self {
        Self { domain_id, adapter }
    }

    fn pid_from_native(native_id: &[u8]) -> Option<ProcessId> {
        decode_native_window_ref(native_id).and_then(|window| window.pid)
    }

    fn single_leaf_snapshot(&self) -> DomainSnapshot {
        DomainSnapshot {
            domain_id: self.domain_id,
            rect: TopologyProvider::rect(self),
            leaves: vec![DomainLeafSnapshot {
                id: 1,
                native_id: encode_native_window_ref(1, None),
                rect: TopologyProvider::rect(self),
                focused: true,
            }],
        }
    }
}

impl TopologyProvider for AppDomainPlugin {
    type NativeId = Vec<u8>;
    type Error = anyhow::Error;

    fn domain_name(&self) -> &'static str {
        self.adapter.adapter_name()
    }

    fn rect(&self) -> Rect {
        Rect {
            x: 0,
            y: 0,
            w: 10000,
            h: 10000,
        }
    }

    fn fetch_layout(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl TopologyModifierImpl for AppDomainPlugin {
    fn focus_impl(&mut self, _id: &Self::NativeId) -> Result<(), Self::Error> {
        Err(anyhow!(
            "adapter '{}' cannot focus by native pane id in legacy bridge mode",
            self.adapter.adapter_name()
        ))
    }

    fn move_impl(&mut self, native_id: &Self::NativeId, dir: Direction) -> Result<(), Self::Error> {
        let pid = Self::pid_from_native(native_id)
            .map(ProcessId::get)
            .context("move requires source pid in native id")?;
        self.adapter
            .move_internal(dir, pid)
            .with_context(|| format!("{} move_internal failed", self.adapter.adapter_name()))
    }

    fn tear_off_impl(
        &mut self,
        native_id: &Self::NativeId,
    ) -> Result<Box<dyn PaneState>, Self::Error> {
        let source_pid = Self::pid_from_native(native_id);
        let preparation = self
            .adapter
            .prepare_merge(source_pid)
            .with_context(|| format!("{} prepare_merge failed", self.adapter.adapter_name()))?;
        Ok(Box::new(AppMergePayload {
            source_pid,
            preparation,
        }))
    }

    fn merge_in_impl(
        &mut self,
        target_native_id: &Self::NativeId,
        dir: Direction,
        payload: Box<dyn PaneState>,
    ) -> Result<Self::NativeId, Self::Error> {
        let target_window_id =
            decode_native_window_ref(target_native_id).map(|window| window.window_id);
        let target_pid = Self::pid_from_native(target_native_id);
        let payload_any = payload.into_any();
        let merge_payload = payload_any
            .downcast::<AppMergePayload>()
            .map_err(|_| anyhow!("unsupported payload for '{}'", self.adapter.adapter_name()))?;
        let preparation = merge_payload
            .preparation
            .with_target_window_hint(target_window_id);
        self.adapter
            .merge_into_target(dir, merge_payload.source_pid, target_pid, preparation)
            .with_context(|| format!("{} merge_into_target failed", self.adapter.adapter_name()))?;
        Ok(target_native_id.clone())
    }
}

impl TilingDomain for AppDomainPlugin {
    fn supported_payload_types(&self) -> &'static [TypeId] {
        if self.adapter.capabilities().merge {
            app_merge_payload_types()
        } else {
            &[]
        }
    }
}

impl ErasedDomain for AppDomainPlugin {
    fn domain_id(&self) -> DomainId {
        self.domain_id
    }

    fn domain_name(&self) -> &'static str {
        self.adapter.adapter_name()
    }

    fn rect(&self) -> Rect {
        TopologyProvider::rect(self)
    }

    fn fetch_snapshot(&mut self) -> Result<DomainSnapshot> {
        Ok(self.single_leaf_snapshot())
    }

    fn supported_payload_types(&self) -> Vec<TypeId> {
        if self.adapter.capabilities().merge {
            vec![TypeId::of::<AppMergePayload>()]
        } else {
            vec![]
        }
    }

    fn tear_off(&mut self, native_id: &[u8]) -> Result<Box<dyn PaneState>> {
        self.tear_off_impl(&native_id.to_vec())
    }

    fn merge_in(
        &mut self,
        target_native_id: &[u8],
        dir: Direction,
        payload: Box<dyn PaneState>,
    ) -> Result<Vec<u8>> {
        self.merge_in_impl(&target_native_id.to_vec(), dir, payload)
    }
}

pub fn domain_id_for_window(
    app_id: Option<&str>,
    pid: Option<ProcessId>,
    title: Option<&str>,
) -> DomainId {
    let app_id = app_id.unwrap_or_default();
    let title = title.unwrap_or_default();
    let owner_pid = pid.map(ProcessId::get).unwrap_or(0);
    if let Some(kind) = apps::resolve_chain(app_id, owner_pid, title)
        .into_iter()
        .map(|adapter| adapter.kind())
        .next()
    {
        return match kind {
            AppKind::Terminal => TERMINAL_DOMAIN_ID,
            AppKind::Editor => EDITOR_DOMAIN_ID,
            AppKind::Browser => WM_DOMAIN_ID,
        };
    }
    WM_DOMAIN_ID
}

pub fn runtime_domains_for_window_manager<W>(wm: &mut W) -> Result<Vec<Box<dyn ErasedDomain>>>
where
    W: WindowManagerAdapter,
{
    let mut domains: Vec<Box<dyn ErasedDomain>> = Vec::new();
    match wm.adapter_name() {
        "niri" => {
            if let Ok(domain) = NiriDomainPlugin::connect(WM_DOMAIN_ID) {
                domains.push(Box::new(domain));
            } else {
                domains.push(Box::new(UnsupportedDomainPlugin::new(WM_DOMAIN_ID, "niri")));
            }
        }
        other => domains.push(Box::new(UnsupportedDomainPlugin::new(WM_DOMAIN_ID, other))),
    }

    domains.push(Box::new(AppDomainPlugin::new(
        TERMINAL_DOMAIN_ID,
        Box::new(TerminalBackend),
    )));
    domains.push(Box::new(AppDomainPlugin::new(
        EDITOR_DOMAIN_ID,
        Box::new(EditorBackend),
    )));

    let (app_id, title, pid) = wm.with_focused_window(|window| {
        Ok((
            window.app_id().unwrap_or("").to_string(),
            window.title().unwrap_or("").to_string(),
            window.pid(),
        ))
    })?;
    let owner_pid = pid.map(ProcessId::get).unwrap_or(0);
    let mut overridden = HashSet::new();
    for adapter in apps::resolve_chain(&app_id, owner_pid, &title) {
        let domain_id = match adapter.kind() {
            AppKind::Terminal => TERMINAL_DOMAIN_ID,
            AppKind::Editor => EDITOR_DOMAIN_ID,
            AppKind::Browser => WM_DOMAIN_ID,
        };
        if overridden.insert(domain_id) {
            domains.push(Box::new(AppDomainPlugin::new(domain_id, adapter)));
        }
    }

    Ok(domains)
}

#[cfg(test)]
mod tests {
    use super::{decode_native_window_ref, domain_id_for_window, encode_native_window_ref};
    use crate::engine::runtime::ProcessId;

    #[test]
    fn native_window_ref_roundtrip_keeps_window_and_pid() {
        let pid = ProcessId::new(4242).expect("pid should be valid");
        let encoded = encode_native_window_ref(99, Some(pid));
        let decoded = decode_native_window_ref(&encoded).expect("native id should decode");
        assert_eq!(decoded.window_id, 99);
        assert_eq!(decoded.pid.map(ProcessId::get), Some(4242));
    }

    #[test]
    fn terminal_app_ids_classify_to_terminal_domain() {
        let domain = domain_id_for_window(Some("org.wezfurlong.wezterm"), None, Some("term"));
        assert_eq!(domain, super::TERMINAL_DOMAIN_ID);
    }
}

#[cfg(test)]
mod transfer_tests {
    use std::any::TypeId;

    use anyhow::{anyhow, Result};

    use super::{
        DomainLeafSnapshot, DomainSnapshot, ErasedDomain, PaneState, PayloadRegistry,
        TransferError, TransferOutcome, TransferPipeline,
    };
    use crate::engine::topology::{Direction, Rect};

    #[derive(Debug)]
    struct BufferState {
        value: String,
    }

    #[derive(Debug)]
    struct ShellState {
        cmd: String,
    }

    #[test]
    fn registry_converts_registered_payload_types() {
        let mut registry = PayloadRegistry::default();
        registry.register(|from: BufferState| ShellState {
            cmd: format!("nvim {}", from.value),
        });

        let result = registry
            .convert(
                Box::new(BufferState {
                    value: "main.rs".into(),
                }),
                TypeId::of::<ShellState>(),
            )
            .expect("converter should be found");

        let any = PaneState::into_any(result);
        let shell = any
            .downcast::<ShellState>()
            .expect("converted payload should downcast");
        assert_eq!(shell.cmd, "nvim main.rs");
    }

    #[test]
    fn registry_returns_structured_error_for_missing_converter() {
        let registry = PayloadRegistry::default();
        let err = match registry.convert(
            Box::new(BufferState {
                value: "main.rs".into(),
            }),
            TypeId::of::<ShellState>(),
        ) {
            Ok(_) => panic!("missing converter should fail"),
            Err(err) => err,
        };
        assert!(matches!(err, TransferError::MissingConverter { .. }));
    }

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
            _dir: Direction,
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
            .transfer_between(&mut source, &[1], &mut target, &[2], Direction::East)
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
            .transfer_between(&mut source, &[1], &mut target, &[2], Direction::East)
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
            .transfer_within(&mut domain, &[1], &[2], Direction::West)
            .expect("within-domain transfer should succeed");

        assert!(matches!(outcome, TransferOutcome::Applied { .. }));
        assert_eq!(
            domain.merged_payload_type,
            Some(TypeId::of::<TargetPayload>())
        );
        assert!(domain.snapshot_reads > 0);
    }
}
