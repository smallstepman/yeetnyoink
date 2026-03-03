use anyhow::{bail, Context, Result};

use crate::adapters::apps::{
    unsupported_operation, AdapterCapabilities, AppKind, DeepApp, MoveDecision, TearResult,
};
use crate::engine::topology::Direction;
use crate::engine::runtime::{self, CommandContext};

pub struct EditorBackend;
pub const ADAPTER_NAME: &str = "editor";
pub const ADAPTER_ALIASES: &[&str] = &["editor", "emacs"];
pub const APP_IDS: &[&str] = &["emacs", "Emacs", "org.gnu.emacs"];

/// Find the focused GUI frame's selected window, then run body in that context.
/// Uses `with-selected-window` which RESTORES the original selection on exit.
/// Use for read-only queries (at_side, window_count, buffer-name, etc).
fn in_focused_window(body: &str) -> String {
    format!(
        "(let* ((--f (car (filtered-frame-list (lambda (f) (frame-focus-state f))))) \
                (--w (and --f (frame-selected-window --f)))) \
           (if --w (with-selected-window --w {body}) (error \"no focused frame\")))"
    )
}

/// Find the focused GUI frame's selected window, select it PERSISTENTLY, then run body.
/// The window/frame selection change sticks after the eval returns.
/// Use for mutations (windmove, swap, delete-window, make-frame, etc).
fn in_focused_window_mut(body: &str) -> String {
    format!(
        "(let* ((--f (car (filtered-frame-list (lambda (f) (frame-focus-state f))))) \
                (--w (and --f (frame-selected-window --f)))) \
           (if --w (progn (select-frame-set-input-focus --f) (select-window --w) {body}) \
             (error \"no focused frame\")))"
    )
}

impl EditorBackend {
    fn eval(expr: &str) -> Result<String> {
        let output = runtime::run_command_output(
            "emacsclient",
            &["--eval", expr],
            &CommandContext {
                adapter: ADAPTER_NAME,
                action: "eval",
                target: None,
            },
        )
        .context("failed to run emacsclient")?;
        if !output.status.success() {
            bail!(
                "emacsclient --eval failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Eval a read-only query in the focused frame's window (temporary context).
    fn eval_in_frame(body: &str) -> Result<String> {
        Self::eval(&in_focused_window(body))
    }

    /// Eval a mutation in the focused frame's window (persistent selection).
    fn eval_in_frame_mut(body: &str) -> Result<String> {
        Self::eval(&in_focused_window_mut(body))
    }

    fn side_symbol(dir: Direction) -> &'static str {
        match dir {
            Direction::West => "left",
            Direction::East => "right",
            Direction::North => "top",
            Direction::South => "bottom",
        }
    }

    fn windmove_fn(dir: Direction) -> &'static str {
        match dir {
            Direction::West => "windmove-left",
            Direction::East => "windmove-right",
            Direction::North => "windmove-up",
            Direction::South => "windmove-down",
        }
    }

    fn windmove_swap_fn(dir: Direction) -> &'static str {
        match dir {
            Direction::West => "windmove-swap-states-left",
            Direction::East => "windmove-swap-states-right",
            Direction::North => "windmove-swap-states-up",
            Direction::South => "windmove-swap-states-down",
        }
    }

    fn split_side(dir: Direction) -> &'static str {
        match dir {
            Direction::West => "left",
            Direction::East => "right",
            Direction::North => "above",
            Direction::South => "below",
        }
    }

    fn resize_delta(dir: Direction, grow: bool, step: i32) -> (i32, bool) {
        let directional = match dir {
            Direction::East | Direction::South => step,
            Direction::West | Direction::North => -step,
        };
        let delta = if grow { directional } else { -directional };
        let horizontal = matches!(dir, Direction::West | Direction::East);
        (delta, horizontal)
    }

    fn at_side(&self, dir: Direction) -> Result<bool> {
        let side = Self::side_symbol(dir);
        let result = Self::eval_in_frame(&format!("(window-at-side-p nil '{side})"))?;
        Ok(result == "t")
    }

    fn window_count(&self) -> Result<u32> {
        let count = Self::eval_in_frame("(length (window-list nil 'no-minibuf))")?;
        Ok(count.parse().unwrap_or(1))
    }
}

impl DeepApp for EditorBackend {
    fn adapter_name(&self) -> &'static str {
        ADAPTER_NAME
    }

    fn kind(&self) -> AppKind {
        AppKind::Editor
    }

    fn capabilities(&self) -> AdapterCapabilities {
        let focus_enabled = crate::config::editor_focus_internal_enabled();
        let move_enabled = crate::config::editor_move_internal_enabled();
        let tear_out_enabled = crate::config::editor_move_tearout_enabled();
        let resize_enabled = crate::config::editor_resize_internal_enabled();
        AdapterCapabilities {
            probe: true,
            focus: focus_enabled,
            move_internal: move_enabled,
            resize_internal: resize_enabled,
            rearrange: move_enabled,
            tear_out: tear_out_enabled,
            merge: true,
        }
    }

    fn can_focus(&self, dir: Direction, _pid: u32) -> Result<bool> {
        if !crate::config::editor_focus_allowed(dir) {
            return Ok(false);
        }
        Ok(!self.at_side(dir)?)
    }

    fn focus(&self, dir: Direction, _pid: u32) -> Result<()> {
        if !crate::config::editor_focus_allowed(dir) {
            return Err(unsupported_operation(self.adapter_name(), "focus"));
        }
        let func = Self::windmove_fn(dir);
        Self::eval_in_frame_mut(&format!("({func})"))?;
        Ok(())
    }

    fn move_decision(&self, dir: Direction, _pid: u32) -> Result<MoveDecision> {
        if !crate::config::editor_move_allowed(dir) {
            return Ok(MoveDecision::Passthrough);
        }
        let win_count = self.window_count()?;
        if win_count <= 1 {
            return Ok(MoveDecision::Passthrough);
        }

        let at_edge = self.at_side(dir)?;
        if !at_edge {
            // There's a neighbor in this direction — swap with it.
            return Ok(MoveDecision::Internal);
        }

        // At the edge in the move direction. Check if there's a neighbor
        // in the perpendicular direction — if so, rearrange the layout
        // rather than tearing out.
        let has_perpendicular_neighbor = match dir {
            Direction::North | Direction::South => {
                !self.at_side(Direction::West)? || !self.at_side(Direction::East)?
            }
            Direction::West | Direction::East => {
                !self.at_side(Direction::North)? || !self.at_side(Direction::South)?
            }
        };

        if has_perpendicular_neighbor {
            Ok(MoveDecision::Rearrange)
        } else if !crate::config::editor_move_tearout_enabled() {
            Ok(MoveDecision::Passthrough)
        } else {
            // At edge and neighbors only along this axis — tear out.
            Ok(MoveDecision::TearOut)
        }
    }

    fn move_internal(&self, dir: Direction, _pid: u32) -> Result<()> {
        if !crate::config::editor_move_allowed(dir) {
            return Err(unsupported_operation(self.adapter_name(), "move_internal"));
        }
        let func = Self::windmove_swap_fn(dir);
        Self::eval_in_frame_mut(&format!("({func})"))?;
        Ok(())
    }

    fn can_resize(&self, dir: Direction, _grow: bool, _pid: u32) -> Result<bool> {
        Ok(crate::config::editor_resize_allowed(dir))
    }

    fn resize_internal(&self, dir: Direction, grow: bool, step: i32, _pid: u32) -> Result<()> {
        if !crate::config::editor_resize_allowed(dir) {
            return Err(unsupported_operation(
                self.adapter_name(),
                "resize_internal",
            ));
        }
        let (delta, horizontal) = Self::resize_delta(dir, grow, step.max(1));
        let horizontal_arg = if horizontal { "t" } else { "nil" };
        let expr = format!("(window-resize nil {delta} {horizontal_arg})");
        Self::eval_in_frame_mut(&expr)?;
        Ok(())
    }

    fn rearrange(&self, dir: Direction, _pid: u32) -> Result<()> {
        let side = Self::split_side(dir);
        // Take the current pane's buffer, delete the pane, then create
        // a new split in the move direction on the remaining pane.
        // e.g. [A|B*] move north: save B, delete B's window, split A above, put B there.
        let expr = format!(
            "(let ((buf (window-buffer))) \
               (delete-window) \
               (let ((new-win (split-window nil nil '{side}))) \
                 (set-window-buffer new-win buf) \
                 (select-window new-win)))"
        );
        Self::eval_in_frame_mut(&expr)?;
        Ok(())
    }

    fn move_out(&self, _dir: Direction, _pid: u32) -> Result<TearResult> {
        if !crate::config::editor_move_tearout_enabled() {
            return Err(unsupported_operation(self.adapter_name(), "move_out"));
        }
        // All in one eval in the focused frame's context:
        // 1. Grab the buffer and current Doom workspace name
        // 2. Delete the emacs window (split)
        // 3. Create a new frame showing that buffer
        // 4. Switch the new frame to the same Doom workspace (persp-mode)
        Self::eval_in_frame_mut(
            "(let* ((buf (window-buffer)) \
                    (ws (and (fboundp '+workspace-current-name) (+workspace-current-name)))) \
               (delete-window) \
               (let* ((f (make-frame)) \
                      (tmp-ws (and ws (with-selected-frame f (+workspace-current-name))))) \
                 (when ws (with-selected-frame f (+workspace/switch-to ws))) \
                 (set-window-buffer (frame-selected-window f) buf) \
                 (when (and tmp-ws (not (equal tmp-ws ws)) (+workspace-exists-p tmp-ws)) \
                   (+workspace-kill tmp-ws t))))",
        )?;
        Ok(TearResult {
            spawn_command: None,
        })
    }

    fn merge_into(&self, dir: Direction, _source_pid: u32) -> Result<()> {
        let side = Self::split_side(dir.opposite());
        // All from the source frame's focused window context:
        // 1. Grab buffer
        // 2. Find the other visible frame (the merge target)
        // 3. In the target frame, create a split and show the buffer
        // 4. Delete the source frame
        let expr = format!(
            "(let* ((buf (window-buffer)) \
                    (src (selected-frame)) \
                    (target (car (delq src (filtered-frame-list #'frame-visible-p))))) \
               (when target \
                 (with-selected-frame target \
                   (let ((new-win (split-window nil nil '{side}))) \
                     (set-window-buffer new-win buf) \
                     (select-window new-win))) \
                 (delete-frame src)))"
        );
        Self::eval_in_frame_mut(&expr)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::EditorBackend;
    use crate::adapters::apps::DeepApp;

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::utils::env_guard()
    }

    #[test]
    fn declares_explicit_capability_contract() {
        let app = EditorBackend;
        let caps = DeepApp::capabilities(&app);
        assert!(caps.probe);
        assert!(caps.focus);
        assert!(caps.move_internal);
        assert!(caps.resize_internal);
        assert!(caps.rearrange);
        assert!(caps.tear_out);
        assert!(caps.merge);
    }

    #[test]
    fn config_can_disable_resize_capability() {
        let _guard = env_guard();
        let base = std::env::temp_dir().join(format!(
            "niri-deep-emacs-resize-config-{}",
            std::process::id()
        ));
        let config_dir = base.join("niri-deep");
        std::fs::create_dir_all(&config_dir).expect("config dir should be creatable");
        std::fs::write(
            config_dir.join("config.toml"),
            r#"
[app.editor.emacs]
enabled = true

[app.editor.emacs.resize.internal_panes]
enabled = false
"#,
        )
        .expect("config file should be writable");

        let old_config_dir = std::env::var_os("XDG_CONFIG_DIR");
        std::env::set_var("XDG_CONFIG_DIR", &base);
        crate::config::prepare().expect("config should load");

        let app = EditorBackend;
        let caps = DeepApp::capabilities(&app);
        assert!(!caps.resize_internal);

        if let Some(value) = old_config_dir {
            std::env::set_var("XDG_CONFIG_DIR", value);
        } else {
            std::env::remove_var("XDG_CONFIG_DIR");
        }
        crate::config::prepare().expect("config should reload");
        let _ = std::fs::remove_dir_all(base);
    }
}
