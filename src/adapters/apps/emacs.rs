use anyhow::{bail, Context, Result};

use crate::engine::contracts::{
    AdapterCapabilities, AppAdapter, AppKind, MergeExecutionMode, MergePreparation, MoveDecision,
    TearResult, TopologyHandler,
};
use crate::engine::runtime::{self, CommandContext, ProcessId};
use crate::engine::topology::Direction;

pub struct EmacsBackend;
pub const APP_IDS: &[&str] = &["emacs", "Emacs", "org.gnu.emacs"];
pub const ADAPTER_NAME: &str = "editor";
pub const ADAPTER_ALIASES: &[&str] = &["emacs", "editor"];

struct EmacsMergePreparation {
    frame_id: String,
}

fn emacs_eval(expr: &str) -> Result<String> {
    let output = runtime::run_command_output(
        "emacsclient",
        &["--eval", expr],
        &CommandContext::new(ADAPTER_NAME, "eval"),
    )
    .context("failed to run emacsclient")?;
    if !output.status.success() {
        bail!(
            "emacsclient --eval failed: {}",
            runtime::stderr_text(&output)
        );
    }
    Ok(runtime::stdout_text(&output))
}

/// Find the focused GUI frame's selected window, then run body in that context.
/// Uses `with-selected-window` which RESTORES the original selection on exit.
/// Use for read-only queries (at_side, window_count, buffer-name, etc).
/// Eval a read-only query in the focused frame's window (temporary context).
fn eval_in_frame(body: &str) -> Result<String> {
    emacs_eval(&format!(
        "(let* ((--f (car (filtered-frame-list (lambda (f) (frame-focus-state f))))) \
                (--w (and --f (frame-selected-window --f)))) \
           (if --w (with-selected-window --w {body}) (error \"no focused frame\")))"
    ))
}

/// Find the focused GUI frame's selected window, select it PERSISTENTLY, then run body.
/// The window/frame selection change sticks after the eval returns.
/// Use for mutations (windmove, swap, delete-window, make-frame, etc).
/// Eval a mutation in the focused frame's window (persistent selection).
fn eval_in_frame_mut(body: &str) -> Result<String> {
    emacs_eval(&format!(
        "(let* ((--f (car (filtered-frame-list (lambda (f) (frame-focus-state f))))) \
                (--w (and --f (frame-selected-window --f)))) \
           (if --w (progn (select-frame-set-input-focus --f) (select-window --w) {body}) \
             (error \"no focused frame\")))"
    ))
}

impl AppAdapter for EmacsBackend {
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

    fn eval(&self, expression: &str, _pid: Option<ProcessId>) -> Result<String> {
        emacs_eval(expression)
    }
}

impl TopologyHandler for EmacsBackend {
    fn at_side(&self, dir: Direction, _pid: u32) -> Result<bool> {
        let side = dir.positional();
        let result = eval_in_frame(&format!("(window-at-side-p nil '{side})"))?;
        Ok(result == "t")
    }

    fn window_count(&self, _pid: u32) -> Result<u32> {
        let count = eval_in_frame("(length (window-list nil 'no-minibuf))")?;
        Ok(count.parse().unwrap_or(1))
    }

    fn can_focus(&self, dir: Direction, pid: u32) -> Result<bool> {
        Ok(!self.at_side(dir, pid)?)
    }

    fn move_decision(&self, dir: Direction, pid: u32) -> Result<MoveDecision> {
        let win_count = self.window_count(pid)?;
        if win_count <= 1 {
            return Ok(MoveDecision::Passthrough);
        }

        let at_edge = self.at_side(dir, pid)?;
        if !at_edge {
            return Ok(MoveDecision::Internal);
        }

        let has_perpendicular_neighbor = match dir {
            Direction::North | Direction::South => {
                !self.at_side(Direction::West, pid)? || !self.at_side(Direction::East, pid)?
            }
            Direction::West | Direction::East => {
                !self.at_side(Direction::North, pid)? || !self.at_side(Direction::South, pid)?
            }
        };

        if has_perpendicular_neighbor {
            Ok(MoveDecision::Rearrange)
        } else {
            Ok(MoveDecision::TearOut)
        }
    }

    fn can_resize(&self, _dir: Direction, _grow: bool, _pid: u32) -> Result<bool> {
        Ok(true)
    }
    fn focus(&self, dir: Direction, _pid: u32) -> Result<()> {
        let func = format!("windmove-{}", dir.egocentric());
        eval_in_frame_mut(&format!("({func})"))?;
        Ok(())
    }

    fn move_internal(&self, dir: Direction, _pid: u32) -> Result<()> {
        let func = format!("windmove-swap-states-{}", dir.egocentric());
        eval_in_frame_mut(&format!("({func})"))?;
        Ok(())
    }

    fn resize_internal(&self, dir: Direction, grow: bool, step: i32, _pid: u32) -> Result<()> {
        let (delta, horizontal) = {
            let directional = match dir {
                Direction::East | Direction::South => step.max(1),
                Direction::West | Direction::North => -step.max(1),
            };
            let delta = if grow { directional } else { -directional };
            let horizontal = matches!(dir, Direction::West | Direction::East);
            (delta, horizontal)
        };
        let horizontal_arg = if horizontal { "t" } else { "nil" };
        let expr = format!("(window-resize nil {delta} {horizontal_arg})");
        eval_in_frame_mut(&expr)?;
        Ok(())
    }

    fn rearrange(&self, dir: Direction, _pid: u32) -> Result<()> {
        let side = dir.relational();
        let expr = format!(
            "(let ((buf (window-buffer))) \
               (delete-window) \
               (let ((new-win (split-window nil nil '{side}))) \
                 (set-window-buffer new-win buf) \
                 (select-window new-win)))"
        );
        eval_in_frame_mut(&expr)?;
        Ok(())
    }

    fn move_out(&self, _dir: Direction, _pid: u32) -> Result<TearResult> {
        eval_in_frame_mut(
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
        let side = dir.opposite().relational();
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
        eval_in_frame_mut(&expr)?;
        Ok(())
    }

    fn merge_execution_mode(&self) -> MergeExecutionMode {
        MergeExecutionMode::TargetFocused
    }

    fn prepare_merge(&self, _source_pid: Option<ProcessId>) -> Result<MergePreparation> {
        let frame_id = eval_in_frame_mut(
            "(let ((id (frame-parameter nil 'yeet-and-yoink-frame-id))) \
               (unless id \
                 (setq id (format \"yeet-and-yoink-%d-%d\" (emacs-pid) (random 1000000000))) \
                 (set-frame-parameter nil 'yeet-and-yoink-frame-id id)) \
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
        let focused_is_source = eval_in_frame(&format!(
            "(equal (frame-parameter nil 'yeet-and-yoink-frame-id) \"{frame_id_lit}\")"
        ))? == "t";
        if focused_is_source {
            return TopologyHandler::merge_into(
                self,
                dir,
                source_pid.map(ProcessId::get).unwrap_or(0),
            );
        }

        let side = dir.opposite().relational();
        let expr = format!(
            "(let* ((target (selected-frame)) \
                    (src-id \"{frame_id_lit}\") \
                    (src nil)) \
               (dolist (f (filtered-frame-list #'frame-visible-p)) \
                 (when (and (not (eq f target)) \
                            (equal (frame-parameter f 'yeet-and-yoink-frame-id) src-id)) \
                   (setq src f))) \
               (unless src (error \"source frame id not found\")) \
               (let ((buf (with-selected-frame src (window-buffer (frame-selected-window src))))) \
                 (let ((new-win (split-window nil nil '{side}))) \
                   (set-window-buffer new-win buf) \
                   (select-window new-win)) \
                  (delete-frame src)))"
        );
        eval_in_frame_mut(&expr)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::EmacsBackend;
    use crate::engine::contracts::AppAdapter;

    #[test]
    fn declares_explicit_capability_contract() {
        let app = EmacsBackend;
        let caps = AppAdapter::capabilities(&app);
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
