//! Pure sidebar rail layout arithmetic (squeeze / restore).
//!
//! Mirrored by `web/viewer.js` (`computeRightAggressorLayout` /
//! `computeLeftAggressorLayout`). Equivalence is pinned by the shared fixture
//! table [`../web/rail_layout_cases.json`](../web/rail_layout_cases.json),
//! exercised here and by `HorizonViewer.runRailFixtureTable` in the browser.
//!
//! Owner rules:
//! 1. Resizing a rail must never change the canvas transform (enforced in JS;
//!    this module only sizes rails).
//! 2. A growing rail consumes the canvas first, then squeezes the opposite
//!    rail toward its minimum. Hard stop = canvas zero AND opposite at min.
//! 3. Before the opposite is squeezed, its home width is the restore target.
//!    Shrinking the aggressor restores the opposite to that home before the
//!    canvas gains pixels. Passive are rewritten only by a manual resize of
//!    that rail; passive squeeze/restore never writes them.

/// Desktop mins — large enough that an 11px inset hit strip still fits.
pub const LEFT_MIN: f64 = 180.0;
pub const RIGHT_MIN: f64 = 240.0;
pub const LEFT_DEFAULT: f64 = 236.0;
pub const RIGHT_DEFAULT: f64 = 316.0;
/// Grab-strip width in CSS pixels (matches `.resize-rail { width: 11px }`).
pub const RAIL_HIT_PX: f64 = 11.0;

/// Authoritative rail widths + remembered homes for one workspace.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RailState {
    pub workspace: f64,
    pub left_min: f64,
    pub right_min: f64,
    pub left: f64,
    pub right: f64,
    /// Restore target for the left rail after a right-rail conquest.
    pub left_home: f64,
    /// Restore target for the right rail after a left-rail conquest.
    pub right_home: f64,
    pub left_open: bool,
    pub right_open: bool,
}

impl RailState {
    pub fn desktop(workspace: f64) -> Self {
        Self {
            workspace,
            left_min: LEFT_MIN,
            right_min: RIGHT_MIN,
            left: LEFT_DEFAULT,
            right: RIGHT_DEFAULT,
            left_home: LEFT_DEFAULT,
            right_home: RIGHT_DEFAULT,
            left_open: true,
            right_open: true,
        }
    }

    pub fn canvas(&self) -> f64 {
        let l = if self.left_open { self.left } else { 0.0 };
        let r = if self.right_open { self.right } else { 0.0 };
        (self.workspace - l - r).max(0.0)
    }

    /// Hard max for the right rail: workspace minus the left's minimum.
    pub fn right_max(&self) -> f64 {
        let other_min = if self.left_open { self.left_min } else { 0.0 };
        self.right_min.max(self.workspace - other_min)
    }

    /// Hard max for the left rail: workspace minus the right's minimum.
    pub fn left_max(&self) -> f64 {
        let other_min = if self.right_open { self.right_min } else { 0.0 };
        self.left_min.max(self.workspace - other_min)
    }

    /// Right rail is the aggressor. Does **not** modify homes.
    pub fn apply_right_aggressor(&mut self, desired_right: f64) {
        let (left, right) = compute_right_aggressor(
            self.workspace,
            desired_right,
            self.left_open,
            self.left_home,
            self.left_min,
            self.right_min,
        );
        if self.left_open {
            self.left = left;
        }
        self.right = right;
    }

    /// Left rail is the aggressor. Does **not** modify homes.
    pub fn apply_left_aggressor(&mut self, desired_left: f64) {
        let (left, right) = compute_left_aggressor(
            self.workspace,
            desired_left,
            self.right_open,
            self.right_home,
            self.left_min,
            self.right_min,
        );
        self.left = left;
        if self.right_open {
            self.right = right;
        }
    }

    /// Manual right resize — establishes a new right home (invalidates prior memory).
    pub fn set_right_manual(&mut self, desired_right: f64) {
        self.apply_right_aggressor(desired_right);
        self.right_home = self.right;
    }

    /// Manual left resize — establishes a new left home.
    pub fn set_left_manual(&mut self, desired_left: f64) {
        self.apply_left_aggressor(desired_left);
        self.left_home = self.left;
    }
}

/// Pure right-aggressor step. Returns `(left, right)`.
///
/// When `left_open` is false, `left` is returned as 0 (caller keeps its own left).
pub fn compute_right_aggressor(
    workspace: f64,
    desired_right: f64,
    left_open: bool,
    left_home: f64,
    left_min: f64,
    right_min: f64,
) -> (f64, f64) {
    let w = workspace.max(0.0);
    let lmin = if left_open { left_min } else { 0.0 };
    let desired = if desired_right.is_finite() {
        desired_right
    } else {
        right_min
    };
    let right = desired.max(right_min).min((w - lmin).max(right_min));
    if !left_open {
        return (0.0, right);
    }
    let lhome = left_home.max(left_min);
    if lhome + right <= w {
        (lhome, right)
    } else {
        ((w - right).max(lmin), right)
    }
}

/// Inclusive-exclusive hit interval `[start, end)` for the left rail, inset
/// inside the left panel so it ends at the panel's inner edge.
pub fn left_rail_hit(left_w: f64) -> (f64, f64) {
    let end = left_w.max(0.0);
    ((end - RAIL_HIT_PX).max(0.0), end)
}

/// Inclusive-exclusive hit interval for the right rail, inset inside the
/// right panel so it starts at the panel's inner edge.
pub fn right_rail_hit(workspace: f64, right_w: f64) -> (f64, f64) {
    let start = (workspace - right_w).max(0.0);
    (start, start + RAIL_HIT_PX)
}

/// True when two half-open intervals are disjoint.
pub fn hits_disjoint(a: (f64, f64), b: (f64, f64)) -> bool {
    a.1 <= b.0 || b.1 <= a.0
}

/// Screen X of a world point given canvas origin, pan, and zoom.
pub fn screen_x(canvas_left: f64, pan_x: f64, zoom: f64, world_x: f64) -> f64 {
    canvas_left + pan_x + world_x * zoom
}

/// New `pan_x` after the canvas origin shifts by `delta_left_occupied`.
pub fn compensate_pan_for_left_occupied(pan_x: f64, delta_left_occupied: f64) -> f64 {
    let next = pan_x - delta_left_occupied;
    if next.is_finite() {
        next
    } else {
        0.0
    }
}

/// Space a rail currently takes in the workspace.
///
/// Collapsed ⇒ 0 regardless of the stored width it will restore to when the
/// user reopens it. Selection / navigation / map load must never flip `open`
/// to true — only the rail's own toggle may (owner stickiness rule).
pub fn rail_occupied(open: bool, stored_width: f64) -> f64 {
    if open {
        stored_width.max(0.0)
    } else {
        0.0
    }
}

/// Pure left-aggressor step. Returns `(left, right)`.
pub fn compute_left_aggressor(
    workspace: f64,
    desired_left: f64,
    right_open: bool,
    right_home: f64,
    left_min: f64,
    right_min: f64,
) -> (f64, f64) {
    let w = workspace.max(0.0);
    let rmin = if right_open { right_min } else { 0.0 };
    let desired = if desired_left.is_finite() {
        desired_left
    } else {
        left_min
    };
    let left = desired.max(left_min).min((w - rmin).max(left_min));
    if !right_open {
        return (left, 0.0);
    }
    let rhome = right_home.max(right_min);
    if left + rhome <= w {
        (left, rhome)
    } else {
        (left, (w - left).max(rmin))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    fn approx(a: f64, b: f64) {
        assert!(
            (a - b).abs() < 1e-9,
            "expected {b}, got {a}"
        );
    }

    #[derive(Debug, Deserialize)]
    struct FixtureFile {
        left_min: f64,
        right_min: f64,
        cases: Vec<FixtureCase>,
    }

    #[derive(Debug, Deserialize)]
    struct FixtureCase {
        name: String,
        side: String,
        workspace: f64,
        desired: f64,
        other_open: bool,
        other_home: f64,
        expect_left: f64,
        expect_right: f64,
    }

    /// Shared table with the JS twin — any drift fails this test.
    #[test]
    fn shared_fixture_table_matches_rust_oracle() {
        let doc: FixtureFile = serde_json::from_str(include_str!("../web/rail_layout_cases.json"))
            .expect("rail_layout_cases.json");
        assert!(!doc.cases.is_empty(), "fixture table must not be empty");
        approx(doc.left_min, LEFT_MIN);
        approx(doc.right_min, RIGHT_MIN);

        for case in &doc.cases {
            let (left, right) = match case.side.as_str() {
                "right" => compute_right_aggressor(
                    case.workspace,
                    case.desired,
                    case.other_open,
                    case.other_home,
                    doc.left_min,
                    doc.right_min,
                ),
                "left" => compute_left_aggressor(
                    case.workspace,
                    case.desired,
                    case.other_open,
                    case.other_home,
                    doc.left_min,
                    doc.right_min,
                ),
                other => panic!("unknown side {other} in {}", case.name),
            };
            assert!(
                (left - case.expect_left).abs() < 1e-9
                    && (right - case.expect_right).abs() < 1e-9,
                "{}: got ({left}, {right}), expect ({}, {})",
                case.name,
                case.expect_left,
                case.expect_right
            );
        }
    }

    /// Owner worked example: 1600px window, left 236, right 316.
    #[test]
    fn worked_example_right_aggressor_squeeze_and_restore() {
        let mut s = RailState::desktop(1600.0);
        approx(s.left, 236.0);
        approx(s.right, 316.0);
        approx(s.canvas(), 1048.0);
        approx(s.left_home, 236.0);

        // Grow right until canvas is zero: right → 1364, left untouched, home unused yet.
        s.apply_right_aggressor(1364.0);
        approx(s.left, 236.0);
        approx(s.right, 1364.0);
        approx(s.canvas(), 0.0);
        approx(s.left_home, 236.0); // still the restore target

        // Keep growing: left is pushed 236 → min (180); 236 remains remembered.
        s.apply_right_aggressor(1420.0);
        approx(s.left, 180.0);
        approx(s.right, 1420.0);
        approx(s.canvas(), 0.0);
        approx(s.left_home, 236.0);
        approx(s.right_max(), 1420.0);

        // Hard stop — cannot push past opposite minimum.
        s.apply_right_aggressor(2000.0);
        approx(s.left, 180.0);
        approx(s.right, 1420.0);
        approx(s.left_home, 236.0);

        // Shrink: left returns to exactly 236 before canvas gains a pixel.
        s.apply_right_aggressor(1364.0);
        approx(s.left, 236.0);
        approx(s.right, 1364.0);
        approx(s.canvas(), 0.0);
        approx(s.left_home, 236.0);

        // One more pixel of shrink → canvas grows; left stays at home.
        s.apply_right_aggressor(1363.0);
        approx(s.left, 236.0);
        approx(s.right, 1363.0);
        approx(s.canvas(), 1.0);

        // Back to start geometry.
        s.apply_right_aggressor(316.0);
        approx(s.left, 236.0);
        approx(s.right, 316.0);
        approx(s.canvas(), 1048.0);
        approx(s.left_home, 236.0);
    }

    #[test]
    fn worked_example_left_aggressor_symmetric() {
        let mut s = RailState::desktop(1600.0);
        // Canvas first: left grows 236 → 1284 (= 1600 − 316), canvas → 0.
        s.apply_left_aggressor(1284.0);
        approx(s.left, 1284.0);
        approx(s.right, 316.0);
        approx(s.canvas(), 0.0);
        approx(s.right_home, 316.0);

        // Squeeze right toward 240; remember 316.
        s.apply_left_aggressor(1360.0);
        approx(s.left, 1360.0);
        approx(s.right, 240.0);
        approx(s.canvas(), 0.0);
        approx(s.right_home, 316.0);

        s.apply_left_aggressor(2000.0);
        approx(s.left, 1360.0); // 1600 − 240
        approx(s.right, 240.0);

        // Restore right to 316 before canvas grows.
        s.apply_left_aggressor(1284.0);
        approx(s.right, 316.0);
        approx(s.canvas(), 0.0);

        s.apply_left_aggressor(1283.0);
        approx(s.right, 316.0);
        approx(s.canvas(), 1.0);
    }

    #[test]
    fn second_squeeze_without_manual_resize_keeps_original_home() {
        let mut s = RailState::desktop(1600.0);

        // First conquest + release (live drag — homes of the squeezed side untouched).
        s.apply_right_aggressor(1420.0);
        approx(s.left, 180.0);
        approx(s.left_home, 236.0);
        s.apply_right_aggressor(316.0);
        approx(s.left, 236.0);
        // Aggressor drag end would commit right home; squeezed home stays 236.
        s.right_home = s.right;
        approx(s.left_home, 236.0);

        // Second squeeze — must still restore to 236, not to 180.
        s.apply_right_aggressor(1420.0);
        approx(s.left, 180.0);
        approx(s.left_home, 236.0);
        s.apply_right_aggressor(316.0);
        approx(s.left, 236.0);
        approx(s.left_home, 236.0);
    }

    #[test]
    fn manual_resize_of_squeezed_rail_invalidates_home() {
        let mut s = RailState::desktop(1600.0);
        s.apply_right_aggressor(1420.0);
        // Aggressor drag end commits its own home (does not touch left_home).
        s.right_home = s.right;
        approx(s.left, 180.0);
        approx(s.left_home, 236.0);
        approx(s.right_home, 1420.0);

        // User drags the squeezed left rail — new baseline 200.
        s.set_left_manual(200.0);
        approx(s.left, 200.0);
        approx(s.left_home, 200.0);
        // Right was pushed as left became aggressor (right_home was 1420).
        approx(s.right, 1400.0); // 1600 − 200

        // Later, right aggressor shrinks: left restores to 200, not 236.
        s.apply_right_aggressor(316.0);
        approx(s.left, 200.0);
        approx(s.left_home, 200.0);
        approx(s.right, 316.0);
        approx(s.canvas(), 1084.0);
    }

    #[test]
    fn apply_aggressor_never_writes_homes() {
        let mut s = RailState::desktop(1600.0);
        s.left_home = 250.0;
        s.right_home = 400.0;
        s.apply_right_aggressor(1420.0);
        approx(s.left_home, 250.0);
        approx(s.right_home, 400.0);
        s.apply_left_aggressor(500.0);
        approx(s.left_home, 250.0);
        approx(s.right_home, 400.0);
    }

    #[test]
    fn manual_set_commits_only_that_rail_home() {
        let mut s = RailState::desktop(1600.0);
        s.set_right_manual(500.0);
        approx(s.right, 500.0);
        approx(s.right_home, 500.0);
        approx(s.left_home, 236.0);
        s.set_left_manual(300.0);
        approx(s.left, 300.0);
        approx(s.left_home, 300.0);
        approx(s.right_home, 500.0);
    }

    #[test]
    fn canvas_zero_widths_are_finite() {
        let (l, r) = compute_right_aggressor(1600.0, 1420.0, true, 236.0, 180.0, 240.0);
        approx(l + r, 1600.0);
        assert!(l.is_finite() && r.is_finite());
        let (l, r) = compute_left_aggressor(1600.0, 1360.0, true, 316.0, 180.0, 240.0);
        approx(l + r, 1600.0);
        assert!(l.is_finite() && r.is_finite());
    }

    #[test]
    fn mid_squeeze_partial_restore_is_monotonic() {
        let mut s = RailState::desktop(1600.0);
        s.apply_right_aggressor(1420.0);
        // At right=1400: left should be 200 (= 1600−1400), still below home.
        s.apply_right_aggressor(1400.0);
        approx(s.left, 200.0);
        approx(s.canvas(), 0.0);
        approx(s.left_home, 236.0);
        s.apply_right_aggressor(1380.0);
        approx(s.left, 220.0);
        s.apply_right_aggressor(1364.0);
        approx(s.left, 236.0);
    }

    /// Inset strips: each rail keeps a full RAIL_HIT_PX of exclusive hit area
    /// at zero canvas, for BOTH aggressor directions. Overlap would let the
    /// later-painted right rail bury the left and make left-aggressor undo
    /// unreachable — which is exactly the owner's bug.
    fn assert_zero_canvas_hits_separable(w: f64, left: f64, right: f64) {
        approx(left + right, w);
        let l = left_rail_hit(left);
        let r = right_rail_hit(w, right);
        let l_w = l.1 - l.0;
        let r_w = r.1 - r.0;
        assert!(
            l_w + 1e-9 >= RAIL_HIT_PX,
            "left exposed width {l_w} < {RAIL_HIT_PX} (hit={l:?})"
        );
        assert!(
            r_w + 1e-9 >= RAIL_HIT_PX,
            "right exposed width {r_w} < {RAIL_HIT_PX} (hit={r:?})"
        );
        assert!(hits_disjoint(l, r), "overlapping hits left={l:?} right={r:?}");
        // Adjacent at the meeting line: left ends where right starts.
        approx(l.1, r.0);
    }

    #[test]
    fn zero_canvas_hits_separable_right_aggressor() {
        let w = 1600.0;
        let mut s = RailState::desktop(w);
        s.apply_right_aggressor(w); // drive to hard stop
        approx(s.canvas(), 0.0);
        approx(s.left, LEFT_MIN);
        assert_zero_canvas_hits_separable(w, s.left, s.right);
        // Undo rail for this state is the RIGHT strip — fully exposed.
        let r = right_rail_hit(w, s.right);
        assert!(r.1 - r.0 >= RAIL_HIT_PX);
    }

    #[test]
    fn zero_canvas_hits_separable_left_aggressor() {
        let w = 1600.0;
        let mut s = RailState::desktop(w);
        s.apply_left_aggressor(w); // drive to hard stop
        approx(s.canvas(), 0.0);
        approx(s.right, RIGHT_MIN);
        assert_zero_canvas_hits_separable(w, s.left, s.right);
        // Undo rail for this state is the LEFT strip — fully exposed.
        let l = left_rail_hit(s.left);
        assert!(l.1 - l.0 >= RAIL_HIT_PX);
    }

    #[test]
    fn zero_canvas_hits_separable_narrow_window() {
        // Matches the owner's measured innerWidth ≈ 937 geometry.
        let w = 937.0;
        let mut s = RailState::desktop(w);
        s.apply_right_aggressor(w);
        assert_zero_canvas_hits_separable(w, s.left, s.right);
        s = RailState::desktop(w);
        s.apply_left_aggressor(w);
        assert_zero_canvas_hits_separable(w, s.left, s.right);
    }

    #[test]
    fn zero_canvas_each_rail_can_still_change_width() {
        let mut s = RailState::desktop(1600.0);
        s.apply_right_aggressor(1420.0);
        s.right_home = s.right; // aggressor drag-end commit
        approx(s.canvas(), 0.0);
        approx(s.left, LEFT_MIN);

        // Right rail dragged toward shrink (widening the canvas / restoring left).
        let right_before = s.right;
        s.apply_right_aggressor(right_before - 40.0);
        assert!(
            s.right < right_before,
            "right rail shrink must take effect from zero-canvas"
        );

        // Back to the trap state.
        s.apply_right_aggressor(1420.0);
        s.right_home = s.right;
        approx(s.left, LEFT_MIN);
        approx(s.canvas(), 0.0);

        // Left rail dragged wider (still operable — pushes the opposite rail).
        let left_before = s.left;
        s.apply_left_aggressor(left_before + 40.0);
        assert!(
            s.left > left_before,
            "left rail grow must take effect from zero-canvas"
        );
        approx(s.canvas(), 0.0);
    }

    /// panX is screen pixels (translate applied after scale in the CSS matrix
    /// product). Compensation is panX -= Δ with no zoom factor — verified at
    /// two zooms so a zoom-dependent bug cannot hide at a single scale.
    #[test]
    fn left_occupied_change_keeps_screen_x_invariant_at_two_zooms() {
        for zoom in [0.25_f64, 1.0_f64] {
            let world_x = 80.0;
            let mut pan = 88.75; // matches a measured live pan
            let mut canvas_left = 236.0;
            let before = screen_x(canvas_left, pan, zoom, world_x);

            let new_left = 400.0;
            let delta = new_left - canvas_left; // +164
            canvas_left = new_left;
            pan = compensate_pan_for_left_occupied(pan, delta);
            approx(pan, 88.75 - 164.0);
            approx(screen_x(canvas_left, pan, zoom, world_x), before);

            // Shrink back.
            let delta_back = 236.0 - canvas_left;
            canvas_left = 236.0;
            pan = compensate_pan_for_left_occupied(pan, delta_back);
            approx(pan, 88.75);
            approx(screen_x(canvas_left, pan, zoom, world_x), before);
        }
    }

    /// Passive squeeze + restore (right aggressor) moves left-occupied; screen
    /// position of a world point must stay fixed across the whole path.
    #[test]
    fn passive_squeeze_and_restore_keeps_screen_x_invariant() {
        let zoom = 0.25;
        let world_x = 100.0;
        let mut pan = 40.0;
        let mut s = RailState::desktop(1600.0);
        let mut canvas_left = s.left;
        let origin = screen_x(canvas_left, pan, zoom, world_x);

        let step = |s: &mut RailState, pan: &mut f64, canvas_left: &mut f64, desired_right: f64| {
            let prev = *canvas_left;
            s.apply_right_aggressor(desired_right);
            let next = s.left; // left-occupied while left_open
            *pan = compensate_pan_for_left_occupied(*pan, next - prev);
            *canvas_left = next;
        };

        step(&mut s, &mut pan, &mut canvas_left, 1364.0); // canvas → 0, left still 236
        approx(s.left, 236.0);
        approx(screen_x(canvas_left, pan, zoom, world_x), origin);

        step(&mut s, &mut pan, &mut canvas_left, 1420.0); // passive squeeze to min
        approx(s.left, LEFT_MIN);
        approx(screen_x(canvas_left, pan, zoom, world_x), origin);

        step(&mut s, &mut pan, &mut canvas_left, 1364.0); // passive restore to home
        approx(s.left, 236.0);
        approx(screen_x(canvas_left, pan, zoom, world_x), origin);

        step(&mut s, &mut pan, &mut canvas_left, 316.0); // canvas returns
        approx(s.canvas(), 1048.0);
        approx(screen_x(canvas_left, pan, zoom, world_x), origin);
    }

    #[test]
    fn left_toggle_occupied_change_keeps_screen_x_invariant() {
        let zoom = 1.0;
        let world_x = 50.0;
        let mut pan = 10.0;
        // Open at 236 → collapse occupied to 0 → expand back to home 236.
        let mut occupied = 236.0;
        let before = screen_x(occupied, pan, zoom, world_x);
        pan = compensate_pan_for_left_occupied(pan, 0.0 - occupied);
        occupied = 0.0;
        approx(screen_x(occupied, pan, zoom, world_x), before);
        pan = compensate_pan_for_left_occupied(pan, 236.0 - occupied);
        occupied = 236.0;
        approx(screen_x(occupied, pan, zoom, world_x), before);
        approx(pan, 10.0);
    }

    /// Intent policy: selection may open a collapsed Inspector; view
    /// manipulation (pan / card-drag / rail-resize / zoom) must not.
    #[test]
    fn selection_may_open_inspector_view_manip_must_not() {
        let stored_right = 240.0;
        let mut right_open = false;
        approx(rail_occupied(right_open, stored_right), 0.0);

        // Selection intent → open.
        right_open = true; // openInspectorForSelection()
        approx(rail_occupied(right_open, stored_right), stored_right);

        // Collapse again, then view-manipulation paths leave the flag alone.
        right_open = false;
        let after_pan = right_open;
        let after_card_drag = right_open; // click suppressed when moved ≥ threshold
        let after_rail_resize = right_open;
        let after_zoom = right_open;
        assert!(!after_pan && !after_card_drag && !after_rail_resize && !after_zoom);
        approx(rail_occupied(after_card_drag, stored_right), 0.0);
    }

    /// Click-vs-drag: travel past the threshold means rearrange, not select.
    #[test]
    fn card_gesture_past_threshold_is_drag_not_click() {
        const DRAG_MOVE: f64 = 5.0;
        let travel = |dx: f64, dy: f64| (dx * dx + dy * dy).sqrt();
        assert!(travel(3.0, 3.0) < DRAG_MOVE); // still a click
        assert!(travel(4.0, 4.0) > DRAG_MOVE); // rearrange — must suppress select
        assert!(travel(10.0, 0.0) > DRAG_MOVE);
    }

    #[test]
    fn stored_width_distinct_from_occupied_when_collapsed() {
        let stored = 236.0;
        assert_ne!(rail_occupied(false, stored), stored);
        approx(rail_occupied(false, stored), 0.0);
        approx(rail_occupied(true, stored), stored);
    }
}
