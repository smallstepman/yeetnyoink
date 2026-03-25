use anyhow::{anyhow, Context, Result};

use crate::engine::topology::Direction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilitySupport {
    Native,
    Composed,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatingFocusMode {
    FloatingOnly,
    TilingAndFloating,
    TilingOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectionalCapability {
    pub west: CapabilitySupport,
    pub east: CapabilitySupport,
    pub north: CapabilitySupport,
    pub south: CapabilitySupport,
}

impl DirectionalCapability {
    pub const fn uniform(value: CapabilitySupport) -> Self {
        Self {
            west: value,
            east: value,
            north: value,
            south: value,
        }
    }

    pub const fn for_direction(self, direction: Direction) -> CapabilitySupport {
        match direction {
            Direction::West => self.west,
            Direction::East => self.east,
            Direction::North => self.north,
            Direction::South => self.south,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrimitiveWindowManagerCapabilities {
    pub tear_out_right: bool,
    pub move_column: bool,
    pub consume_into_column_and_move: bool,
    pub set_window_width: bool,
    pub set_window_height: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowManagerCapabilities {
    pub primitives: PrimitiveWindowManagerCapabilities,
    pub tear_out: DirectionalCapability,
    pub resize: DirectionalCapability,
}

impl WindowManagerCapabilities {
    pub const fn none() -> Self {
        Self {
            primitives: PrimitiveWindowManagerCapabilities {
                tear_out_right: false,
                move_column: false,
                consume_into_column_and_move: false,
                set_window_width: false,
                set_window_height: false,
            },
            tear_out: DirectionalCapability::uniform(CapabilitySupport::Unsupported),
            resize: DirectionalCapability::uniform(CapabilitySupport::Unsupported),
        }
    }

    pub fn validate(self) -> Result<()> {
        for direction in [
            Direction::West,
            Direction::East,
            Direction::North,
            Direction::South,
        ] {
            if matches!(
                self.tear_out.for_direction(direction),
                CapabilitySupport::Composed
            ) && !supports_composed_tear_out(self, direction)
            {
                return Err(anyhow!(
                    "invalid capability declaration: tear_out.{direction} is composed but required primitives are missing"
                ));
            }

            if matches!(
                self.resize.for_direction(direction),
                CapabilitySupport::Composed
            ) && !supports_composed_resize(self, direction)
            {
                return Err(anyhow!(
                    "invalid capability declaration: resize.{direction} is composed but required primitives are missing"
                ));
            }
        }

        Ok(())
    }
}

pub fn plan_tear_out(
    capabilities: WindowManagerCapabilities,
    direction: Direction,
) -> CapabilitySupport {
    match capabilities.tear_out.for_direction(direction) {
        CapabilitySupport::Native => CapabilitySupport::Native,
        CapabilitySupport::Composed if supports_composed_tear_out(capabilities, direction) => {
            CapabilitySupport::Composed
        }
        _ => CapabilitySupport::Unsupported,
    }
}

pub fn plan_resize(
    capabilities: WindowManagerCapabilities,
    direction: Direction,
) -> CapabilitySupport {
    match capabilities.resize.for_direction(direction) {
        CapabilitySupport::Native => CapabilitySupport::Native,
        CapabilitySupport::Composed if supports_composed_resize(capabilities, direction) => {
            CapabilitySupport::Composed
        }
        _ => CapabilitySupport::Unsupported,
    }
}

fn supports_composed_tear_out(
    capabilities: WindowManagerCapabilities,
    direction: Direction,
) -> bool {
    if !capabilities.primitives.tear_out_right {
        return false;
    }

    match direction {
        Direction::East => capabilities.primitives.move_column,
        Direction::West => capabilities.primitives.move_column,
        Direction::North | Direction::South => capabilities.primitives.consume_into_column_and_move,
    }
}

fn supports_composed_resize(capabilities: WindowManagerCapabilities, direction: Direction) -> bool {
    match direction {
        Direction::West | Direction::East => capabilities.primitives.set_window_width,
        Direction::North | Direction::South => capabilities.primitives.set_window_height,
    }
}

pub trait WindowManagerCapabilityDescriptor {
    const NAME: &'static str;
    const CAPABILITIES: WindowManagerCapabilities;
    const FLOATING_FOCUS_MODE: FloatingFocusMode;
}

pub fn validate_declared_capabilities<T: WindowManagerCapabilityDescriptor>() -> Result<()> {
    T::CAPABILITIES
        .validate()
        .with_context(|| format!("invalid capabilities for adapter '{}'", T::NAME))
}
