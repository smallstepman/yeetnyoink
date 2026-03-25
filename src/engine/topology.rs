use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::engine::contracts::MoveDecision;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitAxis {
    Horizontal,
    Vertical,
}

impl SplitAxis {
    pub fn select<T>(self, horizontal: T, vertical: T) -> T {
        match self {
            Self::Horizontal => horizontal,
            Self::Vertical => vertical,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ValueEnum, Deserialize, Serialize)]
pub enum Direction {
    #[serde(alias = "Left", alias = "left", alias = "west", alias = "W")]
    West,
    #[serde(alias = "Right", alias = "right", alias = "east", alias = "E")]
    East,
    #[serde(
        alias = "Up",
        alias = "up",
        alias = "north",
        alias = "N",
        alias = "Above",
        alias = "above"
    )]
    North,
    #[serde(
        alias = "Down",
        alias = "down",
        alias = "south",
        alias = "S",
        alias = "Below",
        alias = "below"
    )]
    South,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ValueEnum, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FloatingFocusStrategy {
    RadialCenter,
    TrailingEdgeParallel,
    LeadingEdgeParallel,
    CrossEdgeGap,
    OverlapThenGap,
    RayAngle,
}

impl Direction {
    pub const ALL: [Self; 4] = [Self::West, Self::East, Self::North, Self::South];

    pub fn select<T>(self, west: T, east: T, north: T, south: T) -> T {
        match self {
            Self::West => west,
            Self::East => east,
            Self::North => north,
            Self::South => south,
        }
    }

    pub fn opposite(self) -> Self {
        match self {
            Self::West => Self::East,
            Self::East => Self::West,
            Self::North => Self::South,
            Self::South => Self::North,
        }
    }

    pub const fn axis(self) -> SplitAxis {
        match self {
            Self::West | Self::East => SplitAxis::Horizontal,
            Self::North | Self::South => SplitAxis::Vertical,
        }
    }

    pub const fn axis_name(self) -> &'static str {
        match self.axis() {
            SplitAxis::Horizontal => "horizontal",
            SplitAxis::Vertical => "vertical",
        }
    }

    pub const fn sign(self) -> i32 {
        match self {
            Self::West | Self::North => -1,
            Self::East | Self::South => 1,
        }
    }

    pub const fn axis_directions(self) -> [Self; 2] {
        match self.axis() {
            SplitAxis::Horizontal => [Self::West, Self::East],
            SplitAxis::Vertical => [Self::North, Self::South],
        }
    }

    pub const fn perpendicular_directions(self) -> [Self; 2] {
        match self.axis() {
            SplitAxis::Horizontal => [Self::North, Self::South],
            SplitAxis::Vertical => [Self::West, Self::East],
        }
    }

    pub const fn cardinal(self) -> &'static str {
        match self {
            Self::West => "west",
            Self::East => "east",
            Self::North => "north",
            Self::South => "south",
        }
    }

    /// Positional terms: left/right/top/bottom.
    pub const fn positional(self) -> &'static str {
        match self {
            Self::West => "left",
            Self::East => "right",
            Self::North => "top",
            Self::South => "bottom",
        }
    }

    /// Relational terms: left/right/above/below.
    pub const fn relational(self) -> &'static str {
        match self {
            Self::West => "left",
            Self::East => "right",
            Self::North => "above",
            Self::South => "below",
        }
    }

    /// Egocentric terms: left/right/up/down.
    pub const fn egocentric(self) -> &'static str {
        match self {
            Self::West => "left",
            Self::East => "right",
            Self::North => "up",
            Self::South => "down",
        }
    }

    #[allow(dead_code)]
    pub const fn vectorial(self) -> &'static str {
        match self {
            Self::West => "backward",
            Self::East => "forward",
            Self::North => "upward",
            Self::South => "downward",
        }
    }

    #[allow(dead_code)]
    pub const fn sequential(self) -> &'static str {
        match self {
            Self::West => "previous",
            Self::East => "next",
            Self::North => "higher",
            Self::South => "lower",
        }
    }

    #[allow(dead_code)]
    pub const fn hierarchical(self) -> &'static str {
        match self {
            Self::West => "previous",
            Self::East => "next",
            Self::North => "parent",
            Self::South => "child",
        }
    }

    pub const fn vim_key(self) -> char {
        match self {
            Self::West => 'h',
            Self::East => 'l',
            Self::North => 'k',
            Self::South => 'j',
        }
    }

    pub const fn tmux_flag(self) -> &'static str {
        match self {
            Self::West => "-L",
            Self::East => "-R",
            Self::North => "-U",
            Self::South => "-D",
        }
    }
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.cardinal())
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

    pub fn perp_overlap_len(self, other: Rect, dir: Direction) -> i32 {
        match dir.axis() {
            SplitAxis::Horizontal => (self.y + self.h).min(other.y + other.h) - self.y.max(other.y),
            SplitAxis::Vertical => (self.x + self.w).min(other.x + other.w) - self.x.max(other.x),
        }
        .max(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectedRect<T> {
    pub id: T,
    pub rect: Rect,
}

fn center_point(rect: Rect) -> (i64, i64) {
    (
        i64::from(rect.x) * 2 + i64::from(rect.w),
        i64::from(rect.y) * 2 + i64::from(rect.h),
    )
}

fn directional_half_plane(source: Rect, candidate: Rect, dir: Direction) -> bool {
    let (source_x, source_y) = center_point(source);
    let (candidate_x, candidate_y) = center_point(candidate);
    match dir {
        Direction::West => candidate_x < source_x,
        Direction::East => candidate_x > source_x,
        Direction::North => candidate_y < source_y,
        Direction::South => candidate_y > source_y,
    }
}

fn directional_gap(dir: Direction, source_edge: i32, candidate_edge: i32) -> i32 {
    match dir {
        Direction::West | Direction::North => source_edge - candidate_edge,
        Direction::East | Direction::South => candidate_edge - source_edge,
    }
}

fn edge_for(dir: Direction, rect: Rect, leading: bool) -> i32 {
    rect.leading_edge(if leading { dir } else { dir.opposite() })
}

fn angular_deviation_from_ray(source: Rect, candidate: Rect, dir: Direction) -> f64 {
    let (source_x, source_y) = center_point(source);
    let (candidate_x, candidate_y) = center_point(candidate);
    let (anchor_x, anchor_y) = match dir.axis() {
        SplitAxis::Horizontal => (i64::from(candidate.receiving_edge(dir)) * 2, candidate_y),
        SplitAxis::Vertical => (candidate_x, i64::from(candidate.receiving_edge(dir)) * 2),
    };
    let dx = anchor_x - source_x;
    let dy = anchor_y - source_y;
    let (parallel, perpendicular) = match dir {
        Direction::West => (-dx, dy.abs()),
        Direction::East => (dx, dy.abs()),
        Direction::North => (-dy, dx.abs()),
        Direction::South => (dy, dx.abs()),
    };
    if parallel <= 0 {
        f64::INFINITY
    } else {
        (perpendicular as f64).atan2(parallel as f64)
    }
}

fn perpendicular_offset(source: Rect, candidate: Rect, dir: Direction) -> i64 {
    let (source_x, source_y) = center_point(source);
    let (candidate_x, candidate_y) = center_point(candidate);
    match dir.axis() {
        SplitAxis::Horizontal => (candidate_y - source_y).abs(),
        SplitAxis::Vertical => (candidate_x - source_x).abs(),
    }
}

fn ray_anchor_distance_squared(source: Rect, candidate: Rect, dir: Direction) -> i64 {
    let (source_x, source_y) = center_point(source);
    let (candidate_x, candidate_y) = center_point(candidate);
    let (anchor_x, anchor_y) = match dir.axis() {
        SplitAxis::Horizontal => (i64::from(candidate.receiving_edge(dir)) * 2, candidate_y),
        SplitAxis::Vertical => (candidate_x, i64::from(candidate.receiving_edge(dir)) * 2),
    };
    let dx = anchor_x - source_x;
    let dy = anchor_y - source_y;
    dx * dx + dy * dy
}

#[derive(Debug, Clone, Copy)]
struct CandidateScore {
    primary: f64,
    secondary: f64,
    perpendicular_offset: i64,
}

impl CandidateScore {
    fn is_better_than(self, other: Self) -> bool {
        self.primary.total_cmp(&other.primary).is_lt()
            || (self.primary == other.primary
                && (self.secondary.total_cmp(&other.secondary).is_lt()
                    || (self.secondary == other.secondary
                        && self.perpendicular_offset < other.perpendicular_offset)))
    }
}

pub fn select_closest_in_direction_with_strategy<T>(
    rects: &[DirectedRect<T>],
    source_id: T,
    dir: Direction,
    strategy: FloatingFocusStrategy,
) -> Option<T>
where
    T: Copy + Eq,
{
    let source = rects.iter().find(|rect| rect.id == source_id)?;
    let (source_center_x, source_center_y) = center_point(source.rect);
    let mut best: Option<(T, CandidateScore)> = None;

    for candidate in rects.iter().copied().filter(|rect| rect.id != source_id) {
        if !directional_half_plane(source.rect, candidate.rect, dir) {
            continue;
        }

        let score = match strategy {
            FloatingFocusStrategy::RadialCenter => {
                let angle = angular_deviation_from_ray(source.rect, candidate.rect, dir);
                if !angle.is_finite() || angle > std::f64::consts::FRAC_PI_4 {
                    continue;
                }
                let (candidate_center_x, candidate_center_y) = center_point(candidate.rect);
                let dx = candidate_center_x - source_center_x;
                let dy = candidate_center_y - source_center_y;
                CandidateScore {
                    primary: (dx * dx + dy * dy) as f64,
                    secondary: 0.0,
                    perpendicular_offset: perpendicular_offset(source.rect, candidate.rect, dir),
                }
            }
            FloatingFocusStrategy::TrailingEdgeParallel => CandidateScore {
                primary: f64::from(directional_gap(
                    dir,
                    edge_for(dir, source.rect, false),
                    edge_for(dir, candidate.rect, false),
                )),
                secondary: 0.0,
                perpendicular_offset: perpendicular_offset(source.rect, candidate.rect, dir),
            },
            FloatingFocusStrategy::LeadingEdgeParallel => CandidateScore {
                primary: f64::from(directional_gap(
                    dir,
                    edge_for(dir, source.rect, true),
                    edge_for(dir, candidate.rect, true),
                )),
                secondary: 0.0,
                perpendicular_offset: perpendicular_offset(source.rect, candidate.rect, dir),
            },
            FloatingFocusStrategy::CrossEdgeGap => CandidateScore {
                primary: f64::from(directional_gap(
                    dir,
                    edge_for(dir, source.rect, false),
                    edge_for(dir, candidate.rect, true),
                )),
                secondary: 0.0,
                perpendicular_offset: perpendicular_offset(source.rect, candidate.rect, dir),
            },
            FloatingFocusStrategy::OverlapThenGap => {
                let overlap = source.rect.perp_overlap_len(candidate.rect, dir);
                if overlap <= 0 {
                    continue;
                }
                CandidateScore {
                    primary: f64::from(directional_gap(
                        dir,
                        edge_for(dir, source.rect, true),
                        candidate.rect.receiving_edge(dir),
                    )),
                    secondary: f64::from(-overlap),
                    perpendicular_offset: perpendicular_offset(source.rect, candidate.rect, dir),
                }
            }
            FloatingFocusStrategy::RayAngle => CandidateScore {
                primary: angular_deviation_from_ray(source.rect, candidate.rect, dir),
                secondary: ray_anchor_distance_squared(source.rect, candidate.rect, dir) as f64,
                perpendicular_offset: perpendicular_offset(source.rect, candidate.rect, dir),
            },
        };

        match best {
            Some((_, best_score)) if !score.is_better_than(best_score) => {}
            _ => best = Some((candidate.id, score)),
        }
    }

    best.map(|(id, _)| id)
}

fn select_closest_in_direction_legacy<T>(
    rects: &[DirectedRect<T>],
    source_id: T,
    dir: Direction,
) -> Option<T>
where
    T: Copy + Eq,
{
    let source = rects.iter().find(|rect| rect.id == source_id)?;
    let mut best: Option<(T, i32, i32)> = None;

    for candidate in rects.iter().copied().filter(|rect| rect.id != source_id) {
        let distance = match dir {
            Direction::West if candidate.rect.x + candidate.rect.w <= source.rect.x => {
                source.rect.x - (candidate.rect.x + candidate.rect.w)
            }
            Direction::East if candidate.rect.x >= source.rect.x + source.rect.w => {
                candidate.rect.x - (source.rect.x + source.rect.w)
            }
            Direction::North if candidate.rect.y + candidate.rect.h <= source.rect.y => {
                source.rect.y - (candidate.rect.y + candidate.rect.h)
            }
            Direction::South if candidate.rect.y >= source.rect.y + source.rect.h => {
                candidate.rect.y - (source.rect.y + source.rect.h)
            }
            _ => continue,
        };
        let overlap = source.rect.perp_overlap_len(candidate.rect, dir);
        if overlap <= 0 {
            continue;
        }
        match best {
            Some((_, best_distance, best_overlap))
                if best_distance < distance
                    || (best_distance == distance && best_overlap >= overlap) => {}
            _ => best = Some((candidate.id, distance, overlap)),
        }
    }

    best.map(|(id, _, _)| id)
}

pub fn select_closest_in_direction<T>(
    rects: &[DirectedRect<T>],
    source_id: T,
    dir: Direction,
) -> Option<T>
where
    T: Copy + Eq,
{
    select_closest_in_direction_legacy(rects, source_id, dir)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalLeaf {
    pub id: LeafId,
    pub domain: DomainId,
    pub native_id: Vec<u8>,
    pub rect: Rect,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DirectionalNeighbors {
    pub west: bool,
    pub east: bool,
    pub north: bool,
    pub south: bool,
}

impl DirectionalNeighbors {
    pub fn in_direction(self, dir: Direction) -> bool {
        match dir {
            Direction::West => self.west,
            Direction::East => self.east,
            Direction::North => self.north,
            Direction::South => self.south,
        }
    }

    pub fn has_perpendicular(self, dir: Direction) -> bool {
        match dir {
            Direction::West | Direction::East => self.north || self.south,
            Direction::North | Direction::South => self.west || self.east,
        }
    }

    pub fn set(&mut self, dir: Direction, value: bool) {
        match dir {
            Direction::West => self.west = value,
            Direction::East => self.east = value,
            Direction::North => self.north = value,
            Direction::South => self.south = value,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MoveSurface {
    pub pane_count: u32,
    pub neighbors: DirectionalNeighbors,
    pub supports_rearrange: bool,
}

impl MoveSurface {
    pub fn decision_for(self, dir: Direction) -> MoveDecision {
        if self.pane_count <= 1 {
            return MoveDecision::Passthrough;
        }
        if self.neighbors.in_direction(dir) {
            return MoveDecision::Internal;
        }
        if self.supports_rearrange && self.neighbors.has_perpendicular(dir) {
            return MoveDecision::Rearrange;
        }
        MoveDecision::TearOut
    }
}

#[cfg(test)]
mod tests {
    use super::{
        select_closest_in_direction, select_closest_in_direction_with_strategy, DirectedRect,
        Direction, DirectionalNeighbors, FloatingFocusStrategy, MoveSurface, Rect, SplitAxis,
    };
    use crate::engine::contracts::MoveDecision;

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
        assert_eq!(a.perp_overlap_len(b, Direction::East), 5);
        assert_eq!(a.perp_overlap_len(b, Direction::South), 0);
    }

    #[test]
    fn direction_string_conversions_cover_reference_sets() {
        assert_eq!(Direction::West.positional(), "left");
        assert_eq!(Direction::East.positional(), "right");
        assert_eq!(Direction::North.positional(), "top");
        assert_eq!(Direction::South.positional(), "bottom");

        assert_eq!(
            Direction::West.axis_directions(),
            [Direction::West, Direction::East]
        );
        assert_eq!(
            Direction::West.perpendicular_directions(),
            [Direction::North, Direction::South]
        );
        assert_eq!(
            Direction::North.axis_directions(),
            [Direction::North, Direction::South]
        );
        assert_eq!(
            Direction::North.perpendicular_directions(),
            [Direction::West, Direction::East]
        );
        assert_eq!(SplitAxis::Horizontal.select("h", "v"), "h");
        assert_eq!(SplitAxis::Vertical.select("h", "v"), "v");

        assert_eq!(Direction::North.relational(), "above");
        assert_eq!(Direction::South.relational(), "below");
        assert_eq!(Direction::North.egocentric(), "up");
        assert_eq!(Direction::South.egocentric(), "down");

        assert_eq!(Direction::West.vectorial(), "backward");
        assert_eq!(Direction::East.vectorial(), "forward");
        assert_eq!(Direction::North.sequential(), "higher");
        assert_eq!(Direction::South.sequential(), "lower");
        assert_eq!(Direction::North.hierarchical(), "parent");
        assert_eq!(Direction::South.hierarchical(), "child");
    }

    #[test]
    fn directional_neighbors_report_direction_and_perpendicular_presence() {
        let mut neighbors = DirectionalNeighbors::default();
        neighbors.set(Direction::West, true);
        neighbors.set(Direction::North, true);

        assert!(neighbors.in_direction(Direction::West));
        assert!(!neighbors.in_direction(Direction::East));
        assert!(neighbors.has_perpendicular(Direction::West));
        assert!(neighbors.has_perpendicular(Direction::North));
    }

    #[test]
    fn move_surface_classifies_by_neighbor_and_rearrange_capability() {
        let surface = MoveSurface {
            pane_count: 2,
            neighbors: DirectionalNeighbors {
                west: false,
                east: false,
                north: true,
                south: false,
            },
            supports_rearrange: true,
        };
        assert!(matches!(
            surface.decision_for(Direction::West),
            MoveDecision::Rearrange
        ));

        let without_rearrange = MoveSurface {
            supports_rearrange: false,
            ..surface
        };
        assert!(matches!(
            without_rearrange.decision_for(Direction::West),
            MoveDecision::TearOut
        ));
    }

    #[test]
    fn select_closest_in_direction_prefers_nearest_overlapping_rect() {
        let rects = vec![
            DirectedRect {
                id: 1_u64,
                rect: Rect {
                    x: 10,
                    y: 10,
                    w: 10,
                    h: 10,
                },
            },
            DirectedRect {
                id: 2_u64,
                rect: Rect {
                    x: 0,
                    y: 12,
                    w: 9,
                    h: 8,
                },
            },
            DirectedRect {
                id: 3_u64,
                rect: Rect {
                    x: -20,
                    y: 12,
                    w: 10,
                    h: 8,
                },
            },
        ];
        assert_eq!(
            select_closest_in_direction(&rects, 1, Direction::West),
            Some(2)
        );
    }

    #[test]
    fn select_closest_in_direction_requires_perpendicular_overlap() {
        let rects = vec![
            DirectedRect {
                id: 1_u64,
                rect: Rect {
                    x: 10,
                    y: 10,
                    w: 10,
                    h: 10,
                },
            },
            DirectedRect {
                id: 2_u64,
                rect: Rect {
                    x: 0,
                    y: 100,
                    w: 9,
                    h: 8,
                },
            },
        ];
        assert_eq!(
            select_closest_in_direction(&rects, 1, Direction::West),
            None
        );
    }

    #[test]
    fn select_closest_in_direction_preserves_legacy_tie_break_order() {
        let rects = vec![
            DirectedRect {
                id: 1_u64,
                rect: Rect {
                    x: 100,
                    y: 100,
                    w: 100,
                    h: 100,
                },
            },
            DirectedRect {
                id: 2_u64,
                rect: Rect {
                    x: 0,
                    y: 80,
                    w: 100,
                    h: 70,
                },
            },
            DirectedRect {
                id: 3_u64,
                rect: Rect {
                    x: 0,
                    y: 100,
                    w: 100,
                    h: 50,
                },
            },
        ];

        assert_eq!(
            select_closest_in_direction(&rects, 1, Direction::West),
            Some(2)
        );
    }

    #[test]
    fn select_closest_in_direction_excludes_partially_overlapping_east_candidates() {
        let rects = vec![
            DirectedRect {
                id: 1_u64,
                rect: Rect {
                    x: 100,
                    y: 100,
                    w: 100,
                    h: 100,
                },
            },
            DirectedRect {
                id: 2_u64,
                rect: Rect {
                    x: 180,
                    y: 120,
                    w: 80,
                    h: 60,
                },
            },
            DirectedRect {
                id: 3_u64,
                rect: Rect {
                    x: 210,
                    y: 120,
                    w: 80,
                    h: 60,
                },
            },
        ];

        assert_eq!(
            select_closest_in_direction(&rects, 1, Direction::East),
            Some(3)
        );
    }

    #[test]
    fn select_closest_in_direction_excludes_partially_overlapping_south_candidates() {
        let rects = vec![
            DirectedRect {
                id: 1_u64,
                rect: Rect {
                    x: 100,
                    y: 100,
                    w: 100,
                    h: 100,
                },
            },
            DirectedRect {
                id: 2_u64,
                rect: Rect {
                    x: 120,
                    y: 180,
                    w: 60,
                    h: 80,
                },
            },
            DirectedRect {
                id: 3_u64,
                rect: Rect {
                    x: 120,
                    y: 210,
                    w: 60,
                    h: 80,
                },
            },
        ];

        assert_eq!(
            select_closest_in_direction(&rects, 1, Direction::South),
            Some(3)
        );
    }

    #[test]
    fn select_closest_in_direction_with_strategy_distinguishes_radial_and_cross_edge() {
        let rects = vec![
            DirectedRect {
                id: 1_u64,
                rect: Rect {
                    x: 200,
                    y: 100,
                    w: 100,
                    h: 100,
                },
            },
            DirectedRect {
                id: 2_u64,
                rect: Rect {
                    x: 40,
                    y: 80,
                    w: 60,
                    h: 60,
                },
            },
            DirectedRect {
                id: 3_u64,
                rect: Rect {
                    x: 90,
                    y: 150,
                    w: 130,
                    h: 130,
                },
            },
        ];

        assert_eq!(
            select_closest_in_direction_with_strategy(
                &rects,
                1,
                Direction::West,
                FloatingFocusStrategy::RadialCenter,
            ),
            Some(2),
        );
        assert_eq!(
            select_closest_in_direction_with_strategy(
                &rects,
                1,
                Direction::West,
                FloatingFocusStrategy::CrossEdgeGap,
            ),
            Some(3),
        );
    }

    #[test]
    fn select_closest_in_direction_with_strategy_distinguishes_parallel_edge_strategies() {
        let rects = vec![
            DirectedRect {
                id: 1_u64,
                rect: Rect {
                    x: 200,
                    y: 200,
                    w: 80,
                    h: 80,
                },
            },
            DirectedRect {
                id: 2_u64,
                rect: Rect {
                    x: 40,
                    y: 170,
                    w: 120,
                    h: 50,
                },
            },
            DirectedRect {
                id: 3_u64,
                rect: Rect {
                    x: 70,
                    y: 260,
                    w: 80,
                    h: 70,
                },
            },
        ];

        assert_eq!(
            select_closest_in_direction_with_strategy(
                &rects,
                1,
                Direction::West,
                FloatingFocusStrategy::TrailingEdgeParallel,
            ),
            Some(2),
        );
        assert_eq!(
            select_closest_in_direction_with_strategy(
                &rects,
                1,
                Direction::West,
                FloatingFocusStrategy::LeadingEdgeParallel,
            ),
            Some(3),
        );
    }

    #[test]
    fn select_closest_in_direction_with_strategy_distinguishes_overlap_then_gap_and_ray_angle() {
        let rects = vec![
            DirectedRect {
                id: 1_u64,
                rect: Rect {
                    x: 200,
                    y: 100,
                    w: 100,
                    h: 100,
                },
            },
            DirectedRect {
                id: 2_u64,
                rect: Rect {
                    x: 130,
                    y: 180,
                    w: 40,
                    h: 40,
                },
            },
            DirectedRect {
                id: 3_u64,
                rect: Rect {
                    x: 20,
                    y: 120,
                    w: 80,
                    h: 60,
                },
            },
        ];

        assert_eq!(
            select_closest_in_direction_with_strategy(
                &rects,
                1,
                Direction::West,
                FloatingFocusStrategy::OverlapThenGap,
            ),
            Some(2),
        );
        assert_eq!(
            select_closest_in_direction_with_strategy(
                &rects,
                1,
                Direction::West,
                FloatingFocusStrategy::RayAngle,
            ),
            Some(3),
        );
    }

    #[test]
    fn select_closest_in_direction_with_strategy_uses_shared_tiebreakers() {
        let rects = vec![
            DirectedRect {
                id: 1_u64,
                rect: Rect {
                    x: 200,
                    y: 200,
                    w: 100,
                    h: 100,
                },
            },
            DirectedRect {
                id: 2_u64,
                rect: Rect {
                    x: 50,
                    y: 225,
                    w: 50,
                    h: 50,
                },
            },
            DirectedRect {
                id: 3_u64,
                rect: Rect {
                    x: 50,
                    y: 0,
                    w: 50,
                    h: 50,
                },
            },
        ];

        for strategy in [
            FloatingFocusStrategy::RadialCenter,
            FloatingFocusStrategy::TrailingEdgeParallel,
            FloatingFocusStrategy::LeadingEdgeParallel,
            FloatingFocusStrategy::CrossEdgeGap,
            FloatingFocusStrategy::OverlapThenGap,
            FloatingFocusStrategy::RayAngle,
        ] {
            assert_eq!(
                select_closest_in_direction_with_strategy(&rects, 1, Direction::West, strategy),
                Some(2),
                "strategy {strategy:?} should break ties by perpendicular offset",
            );
        }
    }
}
