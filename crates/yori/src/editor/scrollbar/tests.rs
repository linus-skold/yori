//! Scroll interaction and geometry, not widget-presence assertions.

use super::*;
use crate::editor::{GUTTER_WIDTH, PaneDocument};
use gpui_kit::component::Root;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{AppContext, Entity, Modifiers, TestAppContext, VisualTestContext};
use yori_document::Document;

fn harness(cx: &mut TestAppContext) -> (Entity<AlignedEditor>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::appearance::init(cx);
        crate::editor::init(cx);
    });

    let mut editor = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let pane = |name: &str, text: String| {
            PaneDocument::new(
                name.into(),
                Document::from_bytes(text.into_bytes()).unwrap(),
            )
        };
        let view = cx.new(|cx| {
            AlignedEditor::new(
                pane("baseline.txt", String::new()),
                pane("local.txt", "long local line\n".repeat(1_000)),
                window,
                cx,
            )
        });
        editor = Some(view.clone());

        Root::new(view, window, cx)
    });
    cx.update(TestWindowExt::render_frame);
    cx.run_until_parked();

    (editor.unwrap(), cx)
}

#[gpui_kit::test]
fn track_jump_and_drag_outside_the_rail_preserve_selection_and_source(cx: &mut TestAppContext) {
    let (editor, cx) = harness(cx);
    let (rail, selection, source, current) = cx.update(|window, cx| {
        let rows = window.find("rows-viewport").bounds();
        window.click_at(
            "rows-viewport",
            point(rows.size.width / 2.0 + px(GUTTER_WIDTH), px(11.0)),
            cx,
        );
        window.press("home", cx);
        window.input("X", cx);
        window.press("shift-right", cx);
        window.render_frame(cx);

        let view = editor.read(cx);
        (
            window.find("diff-scrollbar").bounds(),
            view.right_selection(),
            view.right.document.text().to_owned(),
            view.navigation.current(&view.alignment),
        )
    });

    let middle = point(rail.center().x, rail.center().y);
    cx.simulate_click(middle, Modifiers::default());
    cx.run_until_parked();

    let grab = cx.update(|window, cx| {
        window.render_frame(cx);
        let view = editor.read(cx);
        let track = view.scroll_track();
        assert!(view.vertical_scroll > track.max_scroll() * 0.4);
        assert!(view.vertical_scroll < track.max_scroll() * 0.6);
        assert_eq!(view.right_selection(), selection);
        assert!(view.focus.is_focused(window));

        let thumb = track.thumb(view.vertical_scroll);
        point(
            rail.center().x,
            rail.top() + px(thumb.start.midpoint(thumb.end)),
        )
    });

    cx.simulate_mouse_down(grab, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    cx.update(TestWindowExt::render_frame);
    let outside = point(rail.left() - px(150.0), rail.bottom() + px(30.0));
    cx.simulate_mouse_move(outside, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(outside, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();

    cx.update(|window, cx| {
        let view = editor.read(cx);
        assert!((view.vertical_scroll - view.scroll_track().max_scroll()).abs() < f32::EPSILON);
        assert_eq!(view.right_selection(), selection);
        assert_eq!(view.right.document.text(), source);
        assert_eq!(view.navigation.current(&view.alignment), current);
        assert!(view.scrollbar_grab.is_none());

        window.press("ctrl-z", cx);
        assert_eq!(
            editor.read(cx).right.document.text(),
            "long local line\n".repeat(1_000)
        );
        assert!(!editor.read(cx).is_dirty());
    });
}

#[gpui_kit::test]
fn resize_and_edit_keep_the_track_aligned_and_clamp_short_documents(cx: &mut TestAppContext) {
    let (editor, cx) = harness(cx);

    for width in [1280.0, 700.0] {
        cx.simulate_resize(size(px(width), px(620.0)));
        cx.run_until_parked();

        cx.update(|window, cx| {
            window.render_frame(cx);
            let rail = window.find("diff-scrollbar").bounds();
            let rows = window.find("rows-viewport").bounds();
            let footer = window.find("editor-footer").bounds();
            let content = window.find("aligned-editor").bounds();

            assert_eq!(rail.left(), rows.right());
            assert_eq!(rail.right(), content.right());
            assert_eq!(rail.top(), rows.top());
            assert_eq!(rail.bottom(), rows.bottom());
            assert_eq!(rail.bottom(), footer.top());
        });
    }

    cx.update(|window, cx| {
        let rows = window.find("rows-viewport").bounds();
        window.click_at(
            "rows-viewport",
            point(rows.size.width / 2.0 + px(GUTTER_WIDTH), px(11.0)),
            cx,
        );
        window.press("ctrl-end", cx);
        assert!(editor.read(cx).vertical_scroll > 0.0);

        window.press("ctrl-a", cx);
        window.press("backspace", cx);
        window.render_frame(cx);

        let view = editor.read(cx);
        let track = view.scroll_track();
        assert!(view.vertical_scroll.abs() < f32::EPSILON);
        assert!(track.max_scroll().abs() < f32::EPSILON);
        assert!(track.bands(&view.alignment, LINE_HEIGHT).is_empty());
    });
}

#[gpui_kit::test]
fn merge_marks_use_projected_spans_not_header_inclusive_controls(cx: &mut TestAppContext) {
    let (editor, cx) = harness(cx);
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            let source = |text: &str| Document::from_bytes(text.as_bytes().to_vec()).unwrap();
            let session = yori_diff::merge::MergeSession::new(
                source("base\nseparator\nold\n"),
                source("local\nseparator\n"),
                source("incoming\nseparator\nnew\n"),
            )
            .unwrap();
            *editor = AlignedEditor::from_merge_session(session, window, cx);
            // A large viewport keeps the expected marker positions in row units.
            let track = ScrollTrack::new(0, LINE_HEIGHT, 1000.0);
            let merge = editor.merge.as_ref().unwrap();
            let marks = merge_scrollbar_marks(merge, track);

            assert_eq!(marks.len(), 2);
            assert_eq!(marks[0].range, LINE_HEIGHT..2.0 * LINE_HEIGHT);
            assert_eq!(marks[1].range, 4.0 * LINE_HEIGHT..5.0 * LINE_HEIGHT);
            assert!(marks[0].current);
            assert!(!marks[1].current);
            assert!(marks.iter().all(|mark| !mark.resolved));

            editor.toggle_merge_base(window, cx);
            editor.merge_mark(yori_diff::merge::ConflictId(0), true, window, cx);
            let merge = editor.merge.as_ref().unwrap();
            let marks = merge_scrollbar_marks(merge, track);

            assert_eq!(marks[0].range, 3.0 * LINE_HEIGHT..4.0 * LINE_HEIGHT);
            assert_eq!(marks[1].range, 6.0 * LINE_HEIGHT..7.0 * LINE_HEIGHT);
            assert!(marks[0].current && marks[0].resolved);
            assert!(!marks[1].current && !marks[1].resolved);
        });
    });
}
