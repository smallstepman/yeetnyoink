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
        Ok(self.move_surface(pid)?.decision_for(dir))
    }

    fn supports_rearrange_decision(&self) -> bool {
        // Emacs can explicitly rearrange panes, but WM-triggered edge moves should
        // prefer tearing the focused buffer out instead of auto-rearranging when
        // perpendicular panes exist.
        false
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
            "(let ((id (frame-parameter nil 'yeetnyoink-frame-id))) \
               (unless id \
                 (setq id (format \"yeetnyoink-%d-%d\" (emacs-pid) (random 1000000000))) \
                 (set-frame-parameter nil 'yeetnyoink-frame-id id)) \
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
            "(equal (frame-parameter nil 'yeetnyoink-frame-id) \"{frame_id_lit}\")"
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
                            (equal (frame-parameter f 'yeetnyoink-frame-id) src-id)) \
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
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::EmacsBackend;
    use crate::engine::contracts::{AppAdapter, MoveDecision, TopologyHandler};
    use crate::engine::topology::Direction;

    static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::utils::env_guard()
    }

    struct EmacsHarness {
        base: PathBuf,
        old_path: Option<OsString>,
        old_log_file: Option<OsString>,
        old_window_count: Option<OsString>,
        old_left: Option<OsString>,
        old_right: Option<OsString>,
        old_top: Option<OsString>,
        old_bottom: Option<OsString>,
    }

    impl EmacsHarness {
        fn new() -> Self {
            let unique = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let base = std::env::temp_dir().join(format!(
                "yeetnyoink-emacs-test-{}-{unique}",
                std::process::id()
            ));
            let bin_dir = base.join("bin");
            let log_file = base.join("emacsclient.log");
            fs::create_dir_all(&bin_dir).expect("failed to create fake bin dir");

            let fake_emacsclient = bin_dir.join("emacsclient");
            fs::write(
                &fake_emacsclient,
                r#"#!/bin/sh
set -eu
expr="${2-}"
printf '%s\n' "$expr" >> "${EMACS_TEST_LOG}"
case "$expr" in
  *"(length (window-list nil 'no-minibuf))"*)
    printf '%s\n' "${EMACS_TEST_WINDOW_COUNT:-1}"
    ;;
  *"window-at-side-p nil 'left"*)
    printf '%s\n' "${EMACS_TEST_AT_LEFT:-nil}"
    ;;
  *"window-at-side-p nil 'right"*)
    printf '%s\n' "${EMACS_TEST_AT_RIGHT:-nil}"
    ;;
  *"window-at-side-p nil 'top"*)
    printf '%s\n' "${EMACS_TEST_AT_TOP:-nil}"
    ;;
  *"window-at-side-p nil 'bottom"*)
    printf '%s\n' "${EMACS_TEST_AT_BOTTOM:-nil}"
    ;;
  *)
    echo "unsupported emacs expr: $expr" >&2
    exit 1
    ;;
esac
"#,
            )
            .expect("failed to write fake emacsclient script");
            let mut permissions = fs::metadata(&fake_emacsclient)
                .expect("failed to stat fake emacsclient script")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&fake_emacsclient, permissions)
                .expect("failed to chmod fake emacsclient script");

            let old_path = std::env::var_os("PATH");
            let old_log_file = std::env::var_os("EMACS_TEST_LOG");
            let old_window_count = std::env::var_os("EMACS_TEST_WINDOW_COUNT");
            let old_left = std::env::var_os("EMACS_TEST_AT_LEFT");
            let old_right = std::env::var_os("EMACS_TEST_AT_RIGHT");
            let old_top = std::env::var_os("EMACS_TEST_AT_TOP");
            let old_bottom = std::env::var_os("EMACS_TEST_AT_BOTTOM");

            let mut path_entries = vec![bin_dir];
            if let Some(ref old) = old_path {
                path_entries.extend(std::env::split_paths(old));
            }
            let path = std::env::join_paths(path_entries).expect("failed to compose PATH");
            std::env::set_var("PATH", path);
            std::env::set_var("EMACS_TEST_LOG", &log_file);

            Self {
                base,
                old_path,
                old_log_file,
                old_window_count,
                old_left,
                old_right,
                old_top,
                old_bottom,
            }
        }

        fn set_window_count(&self, count: u32) {
            std::env::set_var("EMACS_TEST_WINDOW_COUNT", count.to_string());
        }

        fn set_at_side(&self, dir: Direction, at_side: bool) {
            let key = match dir {
                Direction::West => "EMACS_TEST_AT_LEFT",
                Direction::East => "EMACS_TEST_AT_RIGHT",
                Direction::North => "EMACS_TEST_AT_TOP",
                Direction::South => "EMACS_TEST_AT_BOTTOM",
            };
            std::env::set_var(key, if at_side { "t" } else { "nil" });
        }
    }

    impl Drop for EmacsHarness {
        fn drop(&mut self) {
            if let Some(value) = &self.old_path {
                std::env::set_var("PATH", value);
            } else {
                std::env::remove_var("PATH");
            }

            if let Some(value) = &self.old_log_file {
                std::env::set_var("EMACS_TEST_LOG", value);
            } else {
                std::env::remove_var("EMACS_TEST_LOG");
            }

            for (key, value) in [
                ("EMACS_TEST_WINDOW_COUNT", self.old_window_count.as_ref()),
                ("EMACS_TEST_AT_LEFT", self.old_left.as_ref()),
                ("EMACS_TEST_AT_RIGHT", self.old_right.as_ref()),
                ("EMACS_TEST_AT_TOP", self.old_top.as_ref()),
                ("EMACS_TEST_AT_BOTTOM", self.old_bottom.as_ref()),
            ] {
                if let Some(value) = value {
                    std::env::set_var(key, value);
                } else {
                    std::env::remove_var(key);
                }
            }

            let _ = fs::remove_dir_all(&self.base);
        }
    }

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

    #[test]
    fn move_decision_prefers_tearout_even_with_perpendicular_neighbors() {
        let _guard = env_guard();
        let harness = EmacsHarness::new();
        harness.set_window_count(4);
        harness.set_at_side(Direction::East, true);
        harness.set_at_side(Direction::West, false);
        harness.set_at_side(Direction::North, false);
        harness.set_at_side(Direction::South, false);

        let app = EmacsBackend;
        let decision = app
            .move_decision(Direction::East, 0)
            .expect("move_decision should succeed");
        assert_eq!(decision, MoveDecision::TearOut);
        assert!(!TopologyHandler::supports_rearrange_decision(&app));
    }
}
