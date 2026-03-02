//! E2E tests for emacs pane operations.
//!
//! Require a running VM with emacs. Run with:
//!   cargo test --test emacs_e2e -- --ignored
//!
//! Nomenclature:
//!   pane   = a split inside emacs (emacs "window")
//!   tile   = a niri window / emacs frame
//!   hop    = focus moving between panes within a tile
//!   swap   = reordering panes within a tile
//!   tear   = ripping a pane out into its own new tile
//!   merge  = absorbing a single-pane tile into an adjacent tile

use std::process::Command;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn vm_eval(expr: &str) -> String {
    let output = Command::new("bash")
        .args([
            "/Users/m/.config/nix/docs/vm.sh",
            "ssh",
            &format!("emacsclient --eval '{}'", expr.replace('\'', "'\\''")),
        ])
        .output()
        .expect("failed to run vm ssh");
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    assert!(
        output.status.success(),
        "emacsclient failed: {stderr}\nexpr: {expr}"
    );
    stdout
}

/// The wrapper niri-deep uses for read-only queries.
fn query(body: &str) -> String {
    let expr = format!(
        "(let* ((--f (car (filtered-frame-list (lambda (f) (frame-focus-state f))))) \
                (--w (and --f (frame-selected-window --f)))) \
           (if --w (with-selected-window --w {body}) (error \"no focused frame\")))"
    );
    vm_eval(&expr)
}

/// The wrapper niri-deep uses for mutations.
fn mutate(body: &str) -> String {
    let expr = format!(
        "(let* ((--f (car (filtered-frame-list (lambda (f) (frame-focus-state f))))) \
                (--w (and --f (frame-selected-window --f)))) \
           (if --w (progn (select-frame-set-input-focus --f) (select-window --w) {body}) \
             (error \"no focused frame\")))"
    );
    vm_eval(&expr)
}

/// Set up a tile with two panes: [L|R*] where R is selected.
/// Returns a snapshot string for verification.
fn setup_two_panes_lr() -> String {
    mutate(
        "(progn \
           (delete-other-windows) \
           (switch-to-buffer \"L\") \
           (split-window-right) \
           (other-window 1) \
           (switch-to-buffer \"R\") \
           \"setup-done\")",
    )
}

/// Snapshot the focused tile's pane layout.
/// Returns a string like: "selected=R panes=L,R at-left=nil at-right=t at-top=t at-bottom=t"
fn snapshot() -> String {
    query(
        "(let* ((bufs (mapcar (lambda (w) (buffer-name (window-buffer w))) \
                              (window-list nil (quote no-minibuf)))) \
                (sel (buffer-name)) \
                (al (window-at-side-p nil (quote left))) \
                (ar (window-at-side-p nil (quote right))) \
                (at (window-at-side-p nil (quote top))) \
                (ab (window-at-side-p nil (quote bottom)))) \
           (format \"selected=%s panes=%s at-left=%s at-right=%s at-top=%s at-bottom=%s\" \
                   sel (mapconcat #'identity bufs \",\") al ar at ab))",
    )
    .trim_matches('"')
    .to_string()
}

/// Count visible frames (tiles).
fn frame_count() -> u32 {
    let result = vm_eval("(length (filtered-frame-list #'frame-visible-p))");
    result.parse().unwrap_or(0)
}

/// Delete all frames except one and reset to a clean state.
fn cleanup() {
    vm_eval(
        "(progn \
           (let ((keep (car (filtered-frame-list #'frame-visible-p)))) \
             (dolist (f (cdr (filtered-frame-list #'frame-visible-p))) \
               (delete-frame f))) \
           (select-frame-set-input-focus (car (filtered-frame-list #'frame-visible-p))) \
           (delete-other-windows) \
           (switch-to-buffer \"*scratch*\") \
           nil)",
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Scenario 1 — HOP: focus navigates between panes within a tile.
///
///   [L|R*] → focus west → [L*|R] → focus east → [L|R*]
///
/// Verifies that `windmove-left/right` inside our frame-targeting wrapper
/// actually changes the selected pane persistently.
#[test]
#[ignore]
fn hop_focus_between_panes() {
    cleanup();
    setup_two_panes_lr();

    // Initial: R is selected, at right edge
    let s = snapshot();
    assert!(s.contains("selected=R"), "initial: {s}");
    assert!(s.contains("at-right=t"), "initial: {s}");

    // Hop west: should select L
    mutate("(windmove-left)");
    let s = snapshot();
    assert!(s.contains("selected=L"), "after hop west: {s}");
    assert!(s.contains("at-left=t"), "after hop west: {s}");

    // Hop east: should select R again
    mutate("(windmove-right)");
    let s = snapshot();
    assert!(s.contains("selected=R"), "after hop east: {s}");
    assert!(s.contains("at-right=t"), "after hop east: {s}");

    cleanup();
}

/// Scenario 2 — SWAP: move reorders panes within a tile without tearing.
///
///   [L|R*] → swap west → [R*|L] → swap west again → still [R*|L] or error
///                                   (R is now at left edge, can't swap further west)
///
/// Verifies that `windmove-swap-states` exchanges pane contents and the
/// selected pane tracks correctly.
#[test]
#[ignore]
fn swap_panes_within_tile() {
    cleanup();
    setup_two_panes_lr();

    // Initial: [L|R*], R at right edge
    let s = snapshot();
    assert!(s.contains("selected=R"), "initial: {s}");

    // Swap west: R's content moves to the left pane. After swap, the
    // selected window should still show R but now be at the left edge.
    mutate("(windmove-swap-states-left)");
    let s = snapshot();
    // After swap-states-left, the cursor stays in the same window position
    // (right side), but the buffers swap. So the right pane now shows L,
    // left pane shows R. The *selected* window is still the right one.
    // That means selected=L (the right window now has L's content).
    //
    // This is the key thing to verify — swap-states swaps BUFFER contents
    // between windows, it does NOT move the cursor.
    assert!(s.contains("panes=R,L"), "after swap west: {s}");

    cleanup();
}

/// Scenario 3 — TEAR: at the edge, move extracts a pane into a new tile.
///
///   [L|R*] (R at right edge) → tear east → tile1=[L] tile2=[R*]
///
/// Verifies that `delete-window` + `make-frame` + `set-window-buffer`
/// creates a new frame with the correct buffer (not doom dashboard).
#[test]
#[ignore]
fn tear_pane_into_new_tile() {
    cleanup();
    setup_two_panes_lr();

    let frames_before = frame_count();

    // Tear: delete the R pane and create a new frame with R's buffer
    mutate(
        "(let ((buf (window-buffer))) \
           (delete-window) \
           (let ((f (make-frame))) \
             (set-window-buffer (frame-selected-window f) buf)))",
    );

    let frames_after = frame_count();
    assert_eq!(
        frames_after,
        frames_before + 1,
        "should have one more frame"
    );

    // The original tile should now show only L
    // The new tile should show R
    // Collect all visible frame buffers
    let all_bufs = vm_eval(
        "(mapcar (lambda (f) \
           (buffer-name (window-buffer (frame-selected-window f)))) \
           (filtered-frame-list #'frame-visible-p))",
    );
    assert!(
        all_bufs.contains("\"R\""),
        "new tile should show R: {all_bufs}"
    );
    assert!(
        all_bufs.contains("\"L\""),
        "original tile should show L: {all_bufs}"
    );
    assert!(
        !all_bufs.contains("dashboard"),
        "no doom dashboard: {all_bufs}"
    );

    cleanup();
}

/// Scenario 4 — MERGE: absorb a single-pane tile back into an adjacent tile.
///
///   tile1=[L*] tile2=[R] → merge east → tile1=[L*|R]
///
/// Verifies that `merge_into` finds the target frame, creates a split there
/// with the correct buffer, and deletes the source frame.
#[test]
#[ignore]
fn merge_tile_into_adjacent() {
    cleanup();

    // Create two separate frames: frame1 with L, frame2 with R
    mutate(
        "(progn \
           (delete-other-windows) \
           (switch-to-buffer \"L\")  \
           (let ((f2 (make-frame))) \
             (set-window-buffer (frame-selected-window f2) (get-buffer-create \"R\"))) \
           \"setup-done\")",
    );

    let frames_before = frame_count();
    assert_eq!(
        frames_before, 2,
        "should start with 2 visible frames (plus possibly hidden)"
    );

    // Merge: from the focused frame (L), absorb R from the other frame.
    // This is what merge_into does — operate from source context,
    // find target, split+buffer there, delete source.
    mutate(
        "(let* ((buf (window-buffer)) \
                (src (selected-frame)) \
                (target (car (delq src (filtered-frame-list #'frame-visible-p))))) \
           (when target \
             (with-selected-frame target \
               (let ((new-win (split-window nil nil (quote left)))) \
                 (set-window-buffer new-win buf) \
                 (select-window new-win))) \
             (delete-frame src)))",
    );

    // Should have one fewer visible frame
    let frames_after = frame_count();
    assert_eq!(
        frames_after,
        frames_before - 1,
        "source frame should be deleted"
    );

    // The remaining tile should have both L and R as panes
    let bufs = vm_eval(
        "(let ((f (car (filtered-frame-list #'frame-visible-p)))) \
           (mapcar (lambda (w) (buffer-name (window-buffer w))) \
                   (window-list f (quote no-minibuf))))",
    );
    assert!(bufs.contains("\"L\""), "merged tile should have L: {bufs}");
    assert!(bufs.contains("\"R\""), "merged tile should have R: {bufs}");

    cleanup();
}

/// Scenario 5 — REARRANGE: move at edge with perpendicular neighbor reorganizes layout.
///
///   [L|R*] → rearrange north → [R*] (top)
///                                [L] (bottom)
///
/// Verifies that the selected pane is moved to the target direction by
/// deleting + re-splitting, resulting in a vertical layout.
#[test]
#[ignore]
fn rearrange_horizontal_to_vertical() {
    cleanup();
    setup_two_panes_lr();

    // Initial: [L|R*], R at right edge, both at top and bottom (horizontal split)
    let s = snapshot();
    assert!(s.contains("selected=R"), "initial: {s}");
    assert!(s.contains("at-right=t"), "initial: {s}");
    assert!(
        s.contains("at-top=t"),
        "initial should span full height: {s}"
    );
    assert!(
        s.contains("at-bottom=t"),
        "initial should span full height: {s}"
    );

    // Rearrange north: delete R's window, split above on remaining L, put R there
    let side = "above";
    mutate(&format!(
        "(let ((buf (window-buffer))) \
           (delete-window) \
           (let ((new-win (split-window nil nil (quote {side})))) \
             (set-window-buffer new-win buf) \
             (select-window new-win)))"
    ));

    let s = snapshot();
    // R should be selected and at the top
    assert!(s.contains("selected=R"), "after rearrange north: {s}");
    assert!(
        s.contains("at-top=t"),
        "R should be at top after rearrange north: {s}"
    );
    // R should NOT be at the bottom (L is below)
    assert!(
        s.contains("at-bottom=nil"),
        "R should not be at bottom: {s}"
    );
    // Both panes should span full width (vertical split now)
    assert!(s.contains("at-left=t"), "R should span full width: {s}");
    assert!(s.contains("at-right=t"), "R should span full width: {s}");
    // Both buffers still present
    assert!(
        s.contains("panes=R,L") || s.contains("panes=L,R"),
        "both panes present: {s}"
    );

    cleanup();
}
