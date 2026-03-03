use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::adapters::window_managers::{
    validate_declared_capabilities, CapabilitySupport, DirectionalCapability, FocusedWindowView,
    PrimitiveWindowManagerCapabilities, ResizeIntent, WindowManagerCapabilities,
    WindowManagerCapabilityDescriptor, WindowManagerExecution, WindowManagerIntrospection,
    WindowRecord,
};
use crate::engine::runtime::{self, CommandContext, ProcessId};
use crate::engine::topology::Direction;

pub struct I3Adapter;

impl I3Adapter {
    pub fn connect() -> Result<Self> {
        validate_declared_capabilities::<Self>()?;
        Ok(Self)
    }

    fn command_output(action: &'static str, args: &[&str]) -> Result<std::process::Output> {
        runtime::run_command_output(
            "i3-msg",
            args,
            &CommandContext {
                adapter: Self::NAME,
                action,
                target: None,
            },
        )
    }

    fn command_status(action: &'static str, args: &[&str]) -> Result<()> {
        runtime::run_command_status(
            "i3-msg",
            args,
            &CommandContext {
                adapter: Self::NAME,
                action,
                target: None,
            },
        )
    }

    fn direction_name(direction: Direction) -> &'static str {
        match direction {
            Direction::West => "left",
            Direction::East => "right",
            Direction::North => "up",
            Direction::South => "down",
        }
    }

    fn tree(&mut self) -> Result<I3Node> {
        let output = Self::command_output("get_tree", &["-t", "get_tree"])?;
        if !output.status.success() {
            bail!(
                "i3-msg -t get_tree failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        serde_json::from_slice(&output.stdout).context("failed to parse i3 tree json")
    }

    fn windows_from_tree(tree: &I3Node) -> Vec<I3WindowData> {
        let mut windows = Vec::new();
        collect_windows(tree, &mut windows);
        windows
    }

    fn focused_window_data(&mut self) -> Result<I3WindowData> {
        let tree = self.tree()?;
        let windows = Self::windows_from_tree(&tree);
        if let Some(window) = windows.iter().find(|window| window.is_focused).cloned() {
            return Ok(window);
        }
        if let Some(node) = focused_leaf(&tree) {
            return Ok(I3WindowData::from_node(node));
        }
        windows
            .into_iter()
            .next()
            .context("no focused i3 window found")
    }
}

#[derive(Clone)]
struct I3WindowData {
    id: u64,
    app_id: Option<String>,
    title: Option<String>,
    pid: Option<ProcessId>,
    is_focused: bool,
}

impl I3WindowData {
    fn from_node(node: &I3Node) -> Self {
        let app_id = node
            .app_id
            .clone()
            .or_else(|| {
                node.window_properties
                    .as_ref()
                    .and_then(|props| props.class.clone())
            })
            .and_then(non_empty);
        let title = node
            .name
            .clone()
            .or_else(|| {
                node.window_properties
                    .as_ref()
                    .and_then(|props| props.title.clone())
            })
            .and_then(non_empty);
        let pid = node.pid.and_then(ProcessId::new);

        Self {
            id: node.id,
            app_id,
            title,
            pid,
            is_focused: node.focused,
        }
    }
}

#[derive(Clone, Copy)]
pub struct I3FocusedWindow<'a> {
    inner: &'a I3WindowData,
}

impl FocusedWindowView for I3FocusedWindow<'_> {
    fn id(&self) -> u64 {
        self.inner.id
    }

    fn app_id(&self) -> Option<&str> {
        self.inner.app_id.as_deref()
    }

    fn title(&self) -> Option<&str> {
        self.inner.title.as_deref()
    }

    fn pid(&self) -> Option<ProcessId> {
        self.inner.pid
    }

    fn original_tile_index(&self) -> usize {
        1
    }
}

impl WindowManagerCapabilityDescriptor for I3Adapter {
    const NAME: &'static str = "i3";
    const CAPABILITIES: WindowManagerCapabilities = WindowManagerCapabilities {
        primitives: PrimitiveWindowManagerCapabilities {
            tear_out_right: false,
            move_column: false,
            consume_into_column_and_move: false,
            set_window_width: true,
            set_window_height: true,
        },
        tear_out: DirectionalCapability::uniform(CapabilitySupport::Unsupported),
        resize: DirectionalCapability::uniform(CapabilitySupport::Native),
    };
}

impl WindowManagerIntrospection for I3Adapter {
    type FocusedWindow<'a>
        = I3FocusedWindow<'a>
    where
        Self: 'a;

    fn with_focused_window<R>(
        &mut self,
        visit: impl for<'a> FnOnce(Self::FocusedWindow<'a>) -> Result<R>,
    ) -> Result<R> {
        let focused = self.focused_window_data()?;
        visit(I3FocusedWindow { inner: &focused })
    }

    fn windows(&mut self) -> Result<Vec<WindowRecord>> {
        let tree = self.tree()?;
        Ok(Self::windows_from_tree(&tree)
            .into_iter()
            .map(|window| WindowRecord {
                id: window.id,
                app_id: window.app_id,
                title: window.title,
                pid: window.pid,
                is_focused: window.is_focused,
                original_tile_index: 1,
            })
            .collect())
    }
}

impl WindowManagerExecution for I3Adapter {
    fn focus_direction(&mut self, direction: Direction) -> Result<()> {
        Self::command_status("focus", &["focus", Self::direction_name(direction)])
    }

    fn move_direction(&mut self, direction: Direction) -> Result<()> {
        Self::command_status("move", &["move", Self::direction_name(direction)])
    }

    fn move_column(&mut self, direction: Direction) -> Result<()> {
        self.move_direction(direction)
    }

    fn consume_into_column_and_move(
        &mut self,
        direction: Direction,
        _original_tile_index: usize,
    ) -> Result<()> {
        self.move_direction(direction)
    }

    fn resize_with_intent(&mut self, intent: ResizeIntent) -> Result<()> {
        let grow = if intent.grow() { "grow" } else { "shrink" };
        let axis = match intent.direction {
            Direction::West | Direction::East => "width",
            Direction::North | Direction::South => "height",
        };
        let amount = intent.step.abs().max(1).to_string();
        Self::command_status("resize", &["resize", grow, axis, &amount, "px"])
    }

    fn spawn(&mut self, command: Vec<String>) -> Result<()> {
        let joined = command.join(" ");
        Self::command_status("spawn", &["exec", "--no-startup-id", &joined])
    }

    fn focus_window_by_id(&mut self, id: u64) -> Result<()> {
        let criteria = format!("[con_id=\"{id}\"]");
        Self::command_status("focus_window_by_id", &[&criteria, "focus"])
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
struct I3WindowProperties {
    #[serde(default)]
    class: Option<String>,
    #[serde(default)]
    title: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct I3Node {
    id: u64,
    #[serde(default)]
    focused: bool,
    #[serde(default)]
    app_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    pid: Option<u32>,
    #[serde(default)]
    window: Option<u64>,
    #[serde(default)]
    window_properties: Option<I3WindowProperties>,
    #[serde(default)]
    nodes: Vec<I3Node>,
    #[serde(default)]
    floating_nodes: Vec<I3Node>,
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn is_window_leaf(node: &I3Node) -> bool {
    let has_children = !node.nodes.is_empty() || !node.floating_nodes.is_empty();
    !has_children
        && (node.window.is_some()
            || node.app_id.is_some()
            || node.window_properties.is_some()
            || node.pid.is_some())
}

fn collect_windows(node: &I3Node, out: &mut Vec<I3WindowData>) {
    if is_window_leaf(node) {
        out.push(I3WindowData::from_node(node));
        return;
    }

    for child in &node.nodes {
        collect_windows(child, out);
    }
    for child in &node.floating_nodes {
        collect_windows(child, out);
    }
}

fn focused_leaf(node: &I3Node) -> Option<&I3Node> {
    if is_window_leaf(node) && node.focused {
        return Some(node);
    }
    for child in &node.nodes {
        if let Some(found) = focused_leaf(child) {
            return Some(found);
        }
    }
    for child in &node.floating_nodes {
        if let Some(found) = focused_leaf(child) {
            return Some(found);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{collect_windows, focused_leaf, I3Node};

    #[test]
    fn extracts_window_leaves_and_focus_from_tree() {
        let sample = r#"
{
  "id": 1,
  "focused": false,
  "nodes": [
    {
      "id": 2,
      "focused": false,
      "nodes": [
        {
          "id": 10,
          "focused": false,
          "window": 10,
          "app_id": "org.wezfurlong.wezterm",
          "name": "wezterm",
          "pid": 9001,
          "nodes": [],
          "floating_nodes": []
        },
        {
          "id": 11,
          "focused": true,
          "window": 11,
          "app_id": "emacs",
          "name": "init.el",
          "pid": 9002,
          "nodes": [],
          "floating_nodes": []
        }
      ],
      "floating_nodes": []
    }
  ],
  "floating_nodes": []
}
        "#;
        let tree: I3Node = serde_json::from_str(sample).expect("tree json should parse");
        let mut windows = Vec::new();
        collect_windows(&tree, &mut windows);
        assert_eq!(windows.len(), 2);
        assert!(windows.iter().any(|window| window.id == 10));
        assert!(windows
            .iter()
            .any(|window| window.id == 11 && window.is_focused));
        assert_eq!(
            focused_leaf(&tree).map(|node| node.id),
            Some(11),
            "focused leaf should resolve to focused window node"
        );
    }
}
