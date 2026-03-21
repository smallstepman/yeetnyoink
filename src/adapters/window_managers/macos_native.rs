use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceKind {
    Desktop,
    Fullscreen,
    SplitView,
    System,
    StageManagerOpaque,
}

#[cfg_attr(not(test), allow(dead_code))]
impl SpaceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Fullscreen => "fullscreen",
            Self::SplitView => "split_view",
            Self::System => "system",
            Self::StageManagerOpaque => "stage_manager_opaque",
        }
    }
}

const DESKTOP_SPACE_TYPE: i32 = 0;
const FULLSCREEN_SPACE_TYPE: i32 = 4;

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
struct RawSpaceRecord {
    managed_space_id: u64,
    space_type: i32,
    tile_spaces: Vec<u64>,
    has_tile_layout_manager: bool,
    stage_manager_managed: bool,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceSnapshot {
    pub id: u64,
    pub kind: SpaceKind,
    pub is_active: bool,
    pub ordered_window_ids: Option<Vec<u64>>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowSnapshot {
    pub id: u64,
    pub pid: Option<u32>,
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub space_id: u64,
    pub order_index: Option<usize>,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
struct RawWindow {
    id: u64,
    pid: Option<u32>,
    app_id: Option<String>,
    title: Option<String>,
    level: i32,
    visible_index: Option<usize>,
}

#[cfg_attr(not(test), allow(dead_code))]
fn classify_space(raw_space: &RawSpaceRecord) -> SpaceKind {
    if raw_space.stage_manager_managed {
        SpaceKind::StageManagerOpaque
    } else if raw_space.has_tile_layout_manager || !raw_space.tile_spaces.is_empty() {
        SpaceKind::SplitView
    } else if raw_space.space_type == FULLSCREEN_SPACE_TYPE {
        SpaceKind::Fullscreen
    } else if raw_space.space_type == DESKTOP_SPACE_TYPE {
        SpaceKind::Desktop
    } else {
        SpaceKind::System
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn order_active_space_windows(windows: &[RawWindow]) -> Vec<RawWindow> {
    let mut ordered = windows.to_vec();
    ordered.sort_by(|left, right| {
        match (left.visible_index, right.visible_index) {
            (Some(left_index), Some(right_index)) => left_index.cmp(&right_index),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
            .then_with(|| right.level.cmp(&left.level))
            .then_with(|| left.id.cmp(&right.id))
    });
    ordered
}

#[allow(dead_code)]
fn snapshots_for_active_space(space_id: u64, windows: &[RawWindow]) -> Vec<WindowSnapshot> {
    let order_by_window_id = order_active_space_windows(windows)
        .into_iter()
        .enumerate()
        .map(|(index, window)| (window.id, index))
        .collect::<HashMap<_, _>>();

    windows
        .iter()
        .map(|window| WindowSnapshot {
            id: window.id,
            pid: window.pid,
            app_id: window.app_id.clone(),
            title: window.title.clone(),
            space_id,
            order_index: order_by_window_id.get(&window.id).copied(),
        })
        .collect()
}

#[cfg_attr(not(test), allow(dead_code))]
fn snapshots_for_inactive_space(space_id: u64, window_ids: &[u64]) -> Vec<WindowSnapshot> {
    window_ids
        .iter()
        .map(|id| WindowSnapshot {
            id: *id,
            pid: None,
            app_id: None,
            title: None,
            space_id,
            order_index: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_window(id: u64) -> RawWindow {
        RawWindow {
            id,
            pid: None,
            app_id: None,
            title: None,
            level: 0,
            visible_index: None,
        }
    }

    impl RawWindow {
        fn with_level(mut self, level: i32) -> Self {
            self.level = level;
            self
        }

        fn with_visible_index(mut self, visible_index: usize) -> Self {
            self.visible_index = Some(visible_index);
            self
        }
    }

    fn raw_desktop_space(managed_space_id: u64) -> RawSpaceRecord {
        RawSpaceRecord {
            managed_space_id,
            space_type: DESKTOP_SPACE_TYPE,
            tile_spaces: Vec::new(),
            has_tile_layout_manager: false,
            stage_manager_managed: false,
        }
    }

    fn raw_fullscreen_space(managed_space_id: u64) -> RawSpaceRecord {
        RawSpaceRecord {
            managed_space_id,
            space_type: FULLSCREEN_SPACE_TYPE,
            tile_spaces: Vec::new(),
            has_tile_layout_manager: false,
            stage_manager_managed: false,
        }
    }

    fn raw_split_space(managed_space_id: u64, tile_spaces: &[u64]) -> RawSpaceRecord {
        RawSpaceRecord {
            managed_space_id,
            space_type: DESKTOP_SPACE_TYPE,
            tile_spaces: tile_spaces.to_vec(),
            has_tile_layout_manager: true,
            stage_manager_managed: false,
        }
    }

    fn raw_stage_manager_space(managed_space_id: u64) -> RawSpaceRecord {
        RawSpaceRecord {
            managed_space_id,
            space_type: DESKTOP_SPACE_TYPE,
            tile_spaces: Vec::new(),
            has_tile_layout_manager: false,
            stage_manager_managed: true,
        }
    }

    #[test]
    fn classify_space_distinguishes_desktop_fullscreen_split_and_stage_manager() {
        assert_eq!(classify_space(&raw_desktop_space(1)), SpaceKind::Desktop);
        assert_eq!(classify_space(&raw_fullscreen_space(2)), SpaceKind::Fullscreen);
        assert_eq!(classify_space(&raw_split_space(3, &[11, 12])), SpaceKind::SplitView);
        assert_eq!(
            classify_space(&raw_stage_manager_space(4)),
            SpaceKind::StageManagerOpaque
        );
    }

    #[test]
    fn active_space_ordering_prefers_frontmost_visible_windows() {
        let windows = vec![
            raw_window(11).with_level(10).with_visible_index(1),
            raw_window(12).with_level(20).with_visible_index(0),
        ];

        let ordered = order_active_space_windows(&windows);
        assert_eq!(ordered.iter().map(|w| w.id).collect::<Vec<_>>(), vec![12, 11]);
    }

    #[test]
    fn active_space_ordering_uses_window_level_when_visible_order_is_missing() {
        let windows = vec![raw_window(21).with_level(10), raw_window(22).with_level(20)];

        let ordered = order_active_space_windows(&windows);
        assert_eq!(ordered.iter().map(|w| w.id).collect::<Vec<_>>(), vec![22, 21]);
    }

    #[test]
    fn active_space_ordering_prefers_visible_windows_over_fallback_ordering() {
        let windows = vec![raw_window(31).with_level(50), raw_window(32).with_visible_index(0)];

        let ordered = order_active_space_windows(&windows);
        assert_eq!(ordered.iter().map(|w| w.id).collect::<Vec<_>>(), vec![32, 31]);
    }

    #[test]
    fn non_active_space_windows_remain_unordered() {
        let snapshots = snapshots_for_inactive_space(99, &[21, 22]);
        assert!(snapshots.iter().all(|window| window.order_index.is_none()));
    }
}
