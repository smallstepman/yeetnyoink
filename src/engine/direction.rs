use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ValueEnum, Deserialize, Serialize)]
pub enum Direction {
    #[serde(alias = "Left", alias = "left", alias = "west", alias = "W")]
    West,
    #[serde(alias = "Right", alias = "right", alias = "east", alias = "E")]
    East,
    #[serde(alias = "Up", alias = "up", alias = "north", alias = "N")]
    North,
    #[serde(alias = "Down", alias = "down", alias = "south", alias = "S")]
    South,
}

impl Direction {
    pub fn opposite(self) -> Self {
        match self {
            Self::West => Self::East,
            Self::East => Self::West,
            Self::North => Self::South,
            Self::South => Self::North,
        }
    }
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::West => write!(f, "west"),
            Self::East => write!(f, "east"),
            Self::North => write!(f, "north"),
            Self::South => write!(f, "south"),
        }
    }
}
