use anyhow::Result;

use crate::engine::runtime::ProcessId;
use crate::engine::topology::Direction;
use crate::engine::wm::capabilities::WindowManagerCapabilities;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeKind {
    Grow,
    Shrink,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResizeIntent {
    pub direction: Direction,
    pub kind: ResizeKind,
    pub step: i32,
}

impl ResizeIntent {
    pub const fn new(direction: Direction, kind: ResizeKind, step: i32) -> Self {
        Self {
            direction,
            kind,
            step,
        }
    }

    pub const fn grow(self) -> bool {
        matches!(self.kind, ResizeKind::Grow)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowRecord {
    pub id: u64,
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub pid: Option<ProcessId>,
    pub is_focused: bool,
    pub original_tile_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusedWindowRecord {
    pub id: u64,
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub pid: Option<ProcessId>,
    pub original_tile_index: usize,
}

pub trait WindowManagerSession: Send {
    fn adapter_name(&self) -> &'static str;
    fn capabilities(&self) -> WindowManagerCapabilities;
    fn focused_window(&mut self) -> Result<FocusedWindowRecord>;
    fn windows(&mut self) -> Result<Vec<WindowRecord>>;
    fn focus_direction(&mut self, direction: Direction) -> Result<()>;
    fn move_direction(&mut self, direction: Direction) -> Result<()>;
    fn resize_with_intent(&mut self, intent: ResizeIntent) -> Result<()>;
    fn spawn(&mut self, command: Vec<String>) -> Result<()>;
    fn focus_window_by_id(&mut self, id: u64) -> Result<()>;
    fn close_window_by_id(&mut self, id: u64) -> Result<()>;
}

pub trait WindowManagerDomainFactory: Send {
    fn create_domain(
        &self,
        domain_id: crate::engine::topology::DomainId,
    ) -> Result<Box<dyn crate::engine::transfer::ErasedDomain>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowCycleRequest {
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub spawn: Option<String>,
    pub new: bool,
    pub summon: bool,
}

pub trait WindowCycleProvider: Send {
    fn focus_or_cycle(&mut self, request: &WindowCycleRequest) -> Result<()>;
}

pub trait WindowTearOutComposer: Send {
    fn compose_tear_out(&mut self, direction: Direction, source_tile_index: usize) -> Result<()>;
}
