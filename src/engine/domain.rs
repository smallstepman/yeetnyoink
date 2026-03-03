use std::any::TypeId;

use anyhow::Result as AnyResult;

use crate::engine::topology::Direction;
use crate::engine::topology::{DomainId, LeafId, Rect};
use crate::engine::transfer::PaneState;

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

use std::collections::HashSet;
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
        let preparation = match merge_payload.preparation {
            MergePreparation::TerminalMuxSourcePane { pane_id, .. } => {
                MergePreparation::TerminalMuxSourcePane {
                    pane_id,
                    target_window_id,
                }
            }
            other => other,
        };
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

pub fn domain_name_for_id(domain_id: DomainId) -> &'static str {
    match domain_id {
        WM_DOMAIN_ID => "wm",
        TERMINAL_DOMAIN_ID => "terminal",
        EDITOR_DOMAIN_ID => "editor",
        _ => "domain",
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
