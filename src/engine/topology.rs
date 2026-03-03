use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitAxis {
    Horizontal,
    Vertical,
}

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

    pub fn axis(self) -> SplitAxis {
        match self {
            Self::West | Self::East => SplitAxis::Horizontal,
            Self::North | Self::South => SplitAxis::Vertical,
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

pub type DomainId = u64;
pub type LeafId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn leading_edge(self, dir: Direction) -> i32 {
        match dir {
            Direction::East => self.x + self.w,
            Direction::West => self.x,
            Direction::South => self.y + self.h,
            Direction::North => self.y,
        }
    }

    pub fn receiving_edge(self, dir: Direction) -> i32 {
        self.leading_edge(dir.opposite())
    }

    pub fn perp_overlap(self, other: Rect, dir: Direction) -> bool {
        match dir.axis() {
            SplitAxis::Horizontal => self.y < other.y + other.h && self.y + self.h > other.y,
            SplitAxis::Vertical => self.x < other.x + other.w && self.x + self.w > other.x,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalLeaf {
    pub id: LeafId,
    pub domain: DomainId,
    pub native_id: Vec<u8>,
    pub rect: Rect,
}

#[cfg(test)]
mod tests {
    use super::{Direction, Rect};

    #[test]
    fn rect_leading_and_receiving_edges_are_opposites() {
        let rect = Rect {
            x: 10,
            y: 20,
            w: 30,
            h: 40,
        };
        assert_eq!(rect.leading_edge(Direction::East), 40);
        assert_eq!(rect.receiving_edge(Direction::East), 10);
        assert_eq!(rect.leading_edge(Direction::South), 60);
        assert_eq!(rect.receiving_edge(Direction::South), 20);
    }

    #[test]
    fn rect_perp_overlap_uses_axis() {
        let a = Rect {
            x: 0,
            y: 0,
            w: 10,
            h: 10,
        };
        let b = Rect {
            x: 20,
            y: 5,
            w: 10,
            h: 10,
        };
        assert!(a.perp_overlap(b, Direction::East));
        assert!(!a.perp_overlap(b, Direction::South));
    }
}
