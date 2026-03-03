use anyhow::{bail, Context, Result};

use crate::engine::contracts::{
    AdapterCapabilities, AppKind, DeepApp, MergeExecutionMode, MergePreparation, MoveDecision,
    TearResult, TopologyModifier, TopologyProvider,
};
use crate::engine::runtime::{self, CommandContext, ProcessId};
use crate::engine::topology::Direction;

pub struct EmacsBackend;
pub const ADAPTER_NAME: &str = "editor";
pub const ADAPTER_ALIASES: &[&str] = &["emacs", "editor"];
pub const APP_IDS: &[&str] = &["emacs", "Emacs", "org.gnu.emacs"];

struct EmacsMergePreparation {
    frame_id: String,
}

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

impl EmacsBackend {
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

impl DeepApp for EmacsBackend {
    fn adapter_name(&self) -> &'static str {
        ADAPTER_NAME
    }

    fn config_aliases(&self) -> Option<&'static [&'static str]> {
        Some(ADAPTER_ALIASES)
    }

    fn kind(&self) -> AppKind {
        AppKind::Editor
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            probe: true,
            focus: true,
            move_internal: true,
            resize_internal: true,
            rearrange: true,
            tear_out: true,
            merge: true,
        }
    }

    fn can_focus(&self, dir: Direction, _pid: u32) -> Result<bool> {
        Ok(!self.at_side(dir)?)
    }

    fn focus(&self, dir: Direction, _pid: u32) -> Result<()> {
        let func = Self::windmove_fn(dir);
        Self::eval_in_frame_mut(&format!("({func})"))?;
        Ok(())
    }

    fn move_decision(&self, dir: Direction, _pid: u32) -> Result<MoveDecision> {
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
        } else {
            // At edge and neighbors only along this axis — tear out.
            Ok(MoveDecision::TearOut)
        }
    }

    fn move_internal(&self, dir: Direction, _pid: u32) -> Result<()> {
        let func = Self::windmove_swap_fn(dir);
        Self::eval_in_frame_mut(&format!("({func})"))?;
        Ok(())
    }

    fn can_resize(&self, _dir: Direction, _grow: bool, _pid: u32) -> Result<bool> {
        Ok(true)
    }

    fn resize_internal(&self, dir: Direction, grow: bool, step: i32, _pid: u32) -> Result<()> {
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
                 (with-selected-frame f \
                   (when ws (+workspace/switch-to ws)) \
                   (delete-other-windows) \
                   (set-window-buffer (selected-window) buf)) \
                 (when (and tmp-ws (not (equal tmp-ws ws)) (+workspace-exists-p tmp-ws)) \
                   (+workspace-kill tmp-ws t))))",
        )?;
        Ok(TearResult {
            spawn_command: None,
        })
    }

    fn merge_into(&self, dir: Direction, _source_pid: u32) -> Result<()> {
        let side = Self::split_side(dir.opposite());
        let (overlap, ahead, distance, offset) = match dir {
            Direction::West => (
                "(and (< src-top cand-bottom) (> src-bottom cand-top))",
                "(<= cand-right src-left)",
                "(- src-left cand-right)",
                "(abs (- cand-top src-top))",
            ),
            Direction::East => (
                "(and (< src-top cand-bottom) (> src-bottom cand-top))",
                "(>= cand-left src-right)",
                "(- cand-left src-right)",
                "(abs (- cand-top src-top))",
            ),
            Direction::North => (
                "(and (< src-left cand-right) (> src-right cand-left))",
                "(<= cand-bottom src-top)",
                "(- src-top cand-bottom)",
                "(abs (- cand-left src-left))",
            ),
            Direction::South => (
                "(and (< src-left cand-right) (> src-right cand-left))",
                "(>= cand-top src-bottom)",
                "(- cand-top src-bottom)",
                "(abs (- cand-left src-left))",
            ),
        };
        // Merge source buffer into the closest visible frame in the requested
        // direction, matching by directional overlap/distance.
        let expr = format!(
            "(let* ((buf (window-buffer)) \
                    (src (selected-frame)) \
                    (src-pos (frame-position src)) \
                    (src-left (car src-pos)) \
                    (src-top (cdr src-pos)) \
                    (src-right (+ src-left (frame-pixel-width src))) \
                    (src-bottom (+ src-top (frame-pixel-height src))) \
                    (target nil) \
                    (best-dist nil) \
                    (best-offset nil)) \
               (dolist (f (delq src (filtered-frame-list #'frame-visible-p))) \
                 (let* ((pos (frame-position f)) \
                        (cand-left (car pos)) \
                        (cand-top (cdr pos)) \
                        (cand-right (+ cand-left (frame-pixel-width f))) \
                        (cand-bottom (+ cand-top (frame-pixel-height f)))) \
                   (when (and {overlap} {ahead}) \
                     (let ((dist {distance}) \
                           (offset {offset})) \
                       (when (or (null best-dist) \
                                 (< dist best-dist) \
                                 (and (= dist best-dist) \
                                      (or (null best-offset) (< offset best-offset)))) \
                         (setq target f \
                               best-dist dist \
                               best-offset offset)))))) \
               (unless target \
                 (setq target (car (delq src (filtered-frame-list #'frame-visible-p))))) \
               (unless target (error \"no merge target frame\")) \
               (with-selected-frame target \
                 (let ((new-win (split-window nil nil '{side}))) \
                   (set-window-buffer new-win buf) \
                   (select-window new-win))) \
               (delete-frame src))"
        );
        Self::eval_in_frame_mut(&expr)?;
        Ok(())
    }

    fn merge_execution_mode(&self) -> MergeExecutionMode {
        MergeExecutionMode::TargetFocused
    }

    fn prepare_merge(&self, _source_pid: Option<ProcessId>) -> Result<MergePreparation> {
        let frame_id = Self::eval_in_frame_mut(
            "(let ((id (frame-parameter nil 'niri-deep-frame-id))) \
               (unless id \
                 (setq id (format \"niri-deep-%d-%d\" (emacs-pid) (random 1000000000))) \
                 (set-frame-parameter nil 'niri-deep-frame-id id)) \
               id)",
        )?;
        let frame_id = frame_id.trim().trim_matches('"').to_string();
        if frame_id.is_empty() || frame_id == "nil" {
            bail!("failed to capture emacs source frame id");
        }
        Ok(MergePreparation::with_payload(EmacsMergePreparation {
            frame_id,
        }))
    }

    fn merge_into_target(
        &self,
        dir: Direction,
        source_pid: Option<ProcessId>,
        _target_pid: Option<ProcessId>,
        preparation: MergePreparation,
    ) -> Result<()> {
        let frame_id = preparation
            .into_payload::<EmacsMergePreparation>()
            .map(|preparation| preparation.frame_id)
            .context("source emacs frame id missing")?;
        let frame_id_lit = frame_id.replace('\\', "\\\\").replace('\"', "\\\"");
        let focused_is_source = Self::eval_in_frame(&format!(
            "(equal (frame-parameter nil 'niri-deep-frame-id) \"{frame_id_lit}\")"
        ))? == "t";
        if focused_is_source {
            return DeepApp::merge_into(self, dir, source_pid.map(ProcessId::get).unwrap_or(0));
        }

        let side = Self::split_side(dir.opposite());
        let expr = format!(
            "(let* ((target (selected-frame)) \
                    (src-id \"{frame_id_lit}\") \
                    (src nil)) \
               (dolist (f (filtered-frame-list #'frame-visible-p)) \
                 (when (and (not (eq f target)) \
                            (equal (frame-parameter f 'niri-deep-frame-id) src-id)) \
                   (setq src f))) \
               (unless src (error \"source frame id not found\")) \
               (let ((buf (with-selected-frame src (window-buffer (frame-selected-window src))))) \
                 (let ((new-win (split-window nil nil '{side}))) \
                   (set-window-buffer new-win buf) \
                   (select-window new-win)) \
                 (delete-frame src)))"
        );
        Self::eval_in_frame_mut(&expr)?;
        Ok(())
    }
}

impl TopologyProvider for EmacsBackend {}
impl TopologyModifier for EmacsBackend {}

#[cfg(test)]
mod tests {
    use super::EmacsBackend;
    use crate::engine::contracts::DeepApp;

    #[test]
    fn declares_explicit_capability_contract() {
        let app = EmacsBackend;
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
    fn advertises_config_aliases_for_policy_binding() {
        let app = EmacsBackend;
        assert_eq!(app.config_aliases(), Some(super::ADAPTER_ALIASES));
    }
}
