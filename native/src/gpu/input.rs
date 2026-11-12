//! Pointer/keyboard event types, hit testing, and button/checkbox/textarea state machines.
//! Owner: agent D.
//!
//! Contract (fixed):
//! - `PointerEvent { Move { x, y }, Down { x, y }, Up { x, y } }` — logical pixels
//! - `KeyEvent { Character(char), Backspace, Enter, Tab, ArrowLeft, ArrowRight, ArrowUp, ArrowDown }`
//!   (extend as needed for M2)
//! - `hit_test(frame: &types::PaintFrame, x: f32, y: f32) -> Option<u64>` — topmost node key
//!   whose rect/text contains the point (walk ops in paint order, take the last match)

use crate::gpu::types::{DrawOp, PaintFrame, TextRun};

/// Pointer events in logical pixels (the surface layer divides device pixels before dispatch).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PointerEvent {
    Move { x: f32, y: f32 },
    Down { x: f32, y: f32 },
    Up { x: f32, y: f32 },
}

/// Keyboard events. The base set covers M0 navigation; M2 editing adds Delete/Home/End/Escape
/// (caret movement, selection and clipboard live in the backend's text-editing state).
/// The M2 variants are not constructed until the surface maps them.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEvent {
    Character(char),
    Backspace,
    Delete,
    Enter,
    Tab,
    Escape,
    Home,
    End,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
}

/// Text hit-area approximation in fractions of the run's font size. `TextRun.y` is the baseline
/// of the first line, so the box spans roughly [baseline − ascent, baseline + descent]. Horizontal
/// extent is the run's full `max_width` box — good enough for M0 (per-glyph bounds come with M2).
const TEXT_ASCENT: f32 = 0.8;
const TEXT_DESCENT: f32 = 0.25;

/// Topmost interactive node key at `(x, y)`, or `None` when only decoration (node 0) or nothing
/// matches. Walks the ops in paint order and keeps the LAST match, which is the topmost paint.
pub fn hit_test(frame: &PaintFrame, x: f32, y: f32) -> Option<u64> {
    let mut hit = None;
    for op in &frame.ops {
        let node = match op {
            DrawOp::Rect(shape) => {
                if shape.rect.contains(x, y) { shape.node } else { continue }
            }
            DrawOp::Text(run) => {
                if text_contains(run, x, y) { run.node } else { continue }
            }
            // Pre-shaped glyph batches carry no node key; the Text op they replaced already had
            // its chance to match (the backend hit-tests the pre-shape frame anyway).
            DrawOp::Glyphs(_) => continue,
        };
        // node 0 = decoration (borders, backgrounds with no owning node) — never hit-testable.
        if node != 0 {
            hit = Some(node);
        }
    }
    hit
}

fn text_contains(run: &TextRun, x: f32, y: f32) -> bool {
    let top = run.y - run.size * TEXT_ASCENT;
    let bottom = run.y + run.size * TEXT_DESCENT;
    x >= run.x && x < run.x + run.max_width.max(0.0) && y >= top && y < bottom
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu::types::{GlyphBatch, Rect, ShapeRect};

    fn rect_op(node: u64, x: f32, y: f32, w: f32, h: f32) -> DrawOp {
        DrawOp::Rect(ShapeRect {
            node,
            rect: Rect { x, y, w, h },
            radius: 0.0,
            fill: Some([1.0, 0.0, 0.0, 1.0]),
            border: None,
            clip: vec![],
        })
    }

    fn text_op(node: u64, x: f32, y: f32, size: f32) -> DrawOp {
        DrawOp::Text(TextRun {
            node,
            x,
            y,
            max_width: 100.0,
            text: "hello".into(),
            size,
            weight: 400,
            color: [0.0, 0.0, 0.0, 1.0],
            align: crate::gpu::types::TextAlign::Left,
            clip: vec![],
        })
    }

    fn frame(ops: Vec<DrawOp>) -> PaintFrame {
        PaintFrame {
            width: 800,
            height: 600,
            scale: 1.0,
            background: [0.0, 0.0, 0.0, 1.0],
            ops,
        }
    }

    #[test]
    fn hits_topmost_rect_of_two_overlapping() {
        let frame = frame(vec![rect_op(1, 0.0, 0.0, 100.0, 100.0), rect_op(2, 50.0, 50.0, 100.0, 100.0)]);
        // Inside both: node 2 was painted last, so it is on top.
        assert_eq!(hit_test(&frame, 60.0, 60.0), Some(2));
        // Inside only the first.
        assert_eq!(hit_test(&frame, 10.0, 10.0), Some(1));
        // Outside everything.
        assert_eq!(hit_test(&frame, 500.0, 500.0), None);
    }

    #[test]
    fn hit_test_edges_are_exclusive_at_right_and_bottom() {
        let frame = frame(vec![rect_op(1, 0.0, 0.0, 100.0, 100.0)]);
        assert_eq!(hit_test(&frame, 100.0, 50.0), None);
        assert_eq!(hit_test(&frame, 50.0, 100.0), None);
        assert_eq!(hit_test(&frame, 0.0, 0.0), Some(1));
    }

    #[test]
    fn hits_text_bounding_area() {
        // Baseline y = 30, size 14 → box spans y in [30 − 11.2, 30 + 3.5] = [18.8, 33.5].
        let frame = frame(vec![text_op(5, 10.0, 30.0, 14.0)]);
        assert_eq!(hit_test(&frame, 12.0, 28.0), Some(5));
        assert_eq!(hit_test(&frame, 12.0, 33.0), Some(5));
        assert_eq!(hit_test(&frame, 12.0, 15.0), None); // above the ascent
        assert_eq!(hit_test(&frame, 12.0, 40.0), None); // below the descent
        assert_eq!(hit_test(&frame, 5.0, 28.0), None); // left of the run box
    }

    #[test]
    fn text_over_rect_last_wins() {
        // Paint order: rect 1, then text 5 on top of it.
        let frame = frame(vec![rect_op(1, 0.0, 0.0, 200.0, 200.0), text_op(5, 10.0, 30.0, 14.0)]);
        assert_eq!(hit_test(&frame, 15.0, 28.0), Some(5));
        // Rect area outside the text box still resolves to the rect's node.
        assert_eq!(hit_test(&frame, 150.0, 100.0), Some(1));
    }

    #[test]
    fn decoration_node_zero_is_never_hit() {
        let decorated = frame(vec![rect_op(0, 0.0, 0.0, 100.0, 100.0)]);
        assert_eq!(hit_test(&decorated, 50.0, 50.0), None);
        // A real node painted earlier still wins over a later decoration overlap.
        let layered = frame(vec![rect_op(7, 0.0, 0.0, 100.0, 100.0), rect_op(0, 50.0, 50.0, 100.0, 100.0)]);
        assert_eq!(hit_test(&layered, 60.0, 60.0), Some(7));
    }

    #[test]
    fn glyph_batches_are_skipped() {
        let glyphs = frame(vec![DrawOp::Glyphs(GlyphBatch::default())]);
        assert_eq!(hit_test(&glyphs, 10.0, 10.0), None);
    }

    #[test]
    fn empty_frame_never_hits() {
        assert_eq!(hit_test(&frame(vec![]), 5.0, 5.0), None);
    }
}
