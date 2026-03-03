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

    fn tie_breaker_offset(self, other: Rect, dir: Direction) -> i32 {
        match dir.axis() {
            SplitAxis::Horizontal => (other.y - self.y).abs(),
            SplitAxis::Vertical => (other.x - self.x).abs(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainNode {
    pub id: DomainId,
    pub parent: Option<DomainId>,
    pub rect: Rect,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalDomainTree {
    pub domains: Vec<DomainNode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalLeaf {
    pub id: LeafId,
    pub domain: DomainId,
    pub native_id: Vec<u8>,
    pub rect: Rect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalTopology {
    pub tree: GlobalDomainTree,
    pub leaves: Vec<GlobalLeaf>,
    pub focused_leaf: Option<LeafId>,
}

pub fn find_neighbor<'a>(
    all_leaves: &'a [GlobalLeaf],
    focused: &GlobalLeaf,
    dir: Direction,
) -> Option<&'a GlobalLeaf> {
    let my_edge = focused.rect.leading_edge(dir);

    all_leaves
        .iter()
        .filter(|leaf| leaf.id != focused.id)
        .filter(|leaf| {
            let edge = leaf.rect.receiving_edge(dir);
            match dir {
                Direction::East | Direction::South => edge >= my_edge,
                Direction::West | Direction::North => edge <= my_edge,
            }
        })
        .filter(|leaf| focused.rect.perp_overlap(leaf.rect, dir))
        .min_by_key(|leaf| {
            (
                (leaf.rect.receiving_edge(dir) - my_edge).abs(),
                focused.rect.tie_breaker_offset(leaf.rect, dir),
                leaf.id,
            )
        })
}

#[cfg(test)]
mod tests {
    use super::{find_neighbor, Direction, GlobalLeaf, Rect};

    fn leaf(id: u64, domain: u64, rect: Rect) -> GlobalLeaf {
        GlobalLeaf {
            id,
            domain,
            native_id: id.to_le_bytes().to_vec(),
            rect,
        }
    }

    #[test]
    fn solver_prefers_closest_directional_candidate() {
        let focused = leaf(
            1,
            1,
            Rect {
                x: 100,
                y: 100,
                w: 100,
                h: 100,
            },
        );
        let leaves = vec![
            focused.clone(),
            leaf(
                2,
                1,
                Rect {
                    x: 30,
                    y: 100,
                    w: 60,
                    h: 100,
                },
            ),
            leaf(
                3,
                1,
                Rect {
                    x: 0,
                    y: 100,
                    w: 20,
                    h: 100,
                },
            ),
        ];

        let picked = find_neighbor(&leaves, &focused, Direction::West).expect("should pick a leaf");
        assert_eq!(picked.id, 2);
    }

    #[test]
    fn solver_rejects_diagonal_without_overlap() {
        let focused = leaf(
            1,
            1,
            Rect {
                x: 100,
                y: 100,
                w: 100,
                h: 100,
            },
        );
        let leaves = vec![
            focused.clone(),
            leaf(
                2,
                1,
                Rect {
                    x: 30,
                    y: 250,
                    w: 60,
                    h: 60,
                },
            ),
        ];
        assert!(find_neighbor(&leaves, &focused, Direction::West).is_none());
    }
}
