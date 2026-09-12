use super::*;
use crate::editor::{GUTTER_WIDTH, Selection, Side};

fn select_line(
    editor: &Entity<AlignedEditor>,
    side: Side,
    text: &str,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) {
    let view = editor.read(cx);
    let offset = view.document(side).document.text().find(text).unwrap();
    let row = view.row_for_source(side, offset);
    let x = view.pane_left(side) + GUTTER_WIDTH + 2.0;
    let y = display_units(row) * LINE_HEIGHT - view.vertical_scroll + 11.0;

    window.click_at("rows-viewport", point(px(x), px(y)), cx);
    window.press("home", cx);
    window.press("shift-end", cx);
}

#[gpui_kit::test]
fn source_and_result_selections_take_only_timeout_without_legacy_mode(cx: &mut TestAppContext) {
    let (editor, cx) = harness(cx);
    for side in [Side::Incoming, Side::Right] {
        cx.update(|window, cx| {
            let original = editor.read(cx).right.document.text().to_owned();
            select_line(
                &editor,
                side,
                if side == Side::Incoming {
                    "    30"
                } else {
                    "    45"
                },
                window,
                cx,
            );
            let take = editor
                .read(cx)
                .merge_line_take(MergeInput::Incoming)
                .unwrap();
            assert_eq!(
                editor
                    .read(cx)
                    .right
                    .document
                    .copy_range(take.plan.local.clone()),
                "    45\n"
            );
            assert!(editor.read(cx).merge_line_take(MergeInput::Local).is_none());

            window.hover("merge-selected-incoming", cx);
            assert_eq!(
                editor.read(cx).merge.as_ref().unwrap().hovered_lines,
                Some(MergeInput::Incoming)
            );
            window.click("merge-selected-incoming", cx);
            assert_eq!(
                editor.read(cx).right.document.text(),
                original.replacen("    45", "    30", 1)
            );
            assert!(
                !editor
                    .read(cx)
                    .right
                    .document
                    .text()
                    .contains("legacy_mode")
            );
            assert_eq!(
                editor
                    .read(cx)
                    .merge
                    .as_ref()
                    .unwrap()
                    .session
                    .unresolved()
                    .count(),
                3
            );
            assert_synchronized(editor.read(cx));

            window.press("ctrl-z", cx);
            assert_eq!(editor.read(cx).right.document.text(), original);
            window.press("ctrl-shift-z", cx);
            assert!(editor.read(cx).right.document.text().contains("    30"));
            window.press("ctrl-z", cx);
        });
        cx.run_until_parked();
    }
}

#[gpui_kit::test]
fn result_selection_can_choose_either_input_without_reopening_a_resolved_conflict(
    cx: &mut TestAppContext,
) {
    let (editor, cx) = harness(cx);
    cx.update(|window, cx| {
        editor.update(cx, |view, cx| {
            view.merge_mark(ConflictId(1), true, window, cx);
            let start = view.right.document.text().find("45").unwrap();
            view.selection = Some(Selection {
                side: Side::Right,
                anchor: start,
                head: start + 2,
            });
            view.replace_text_in_range(None, "99", window, cx);
        });
        window.render_frame(cx);
        select_line(&editor, Side::Right, "    99", window, cx);
        assert!(editor.read(cx).merge_line_take(MergeInput::Local).is_some());
        assert!(
            editor
                .read(cx)
                .merge_line_take(MergeInput::Incoming)
                .is_some()
        );

        window.click("merge-selected-local", cx);
        assert!(editor.read(cx).right.document.text().contains("    45"));
        assert!(
            editor
                .read(cx)
                .merge
                .as_ref()
                .unwrap()
                .session
                .state(ConflictId(1))
                .unwrap()
                .resolved
        );
        window.press("ctrl-z", cx);
        assert!(editor.read(cx).right.document.text().contains("    99"));
        assert!(
            editor
                .read(cx)
                .merge
                .as_ref()
                .unwrap()
                .session
                .state(ConflictId(1))
                .unwrap()
                .resolved
        );
        assert_synchronized(editor.read(cx));
    });
}

#[gpui_kit::test]
fn local_selection_and_deletion_gaps_use_the_same_narrowed_actions(cx: &mut TestAppContext) {
    let session = MergeSession::new(
        document("head\nvalue = base;\ntail\n"),
        document("head\nvalue = local;\ntail\n"),
        document("head\ntail\n"),
    )
    .unwrap();
    let (editor, cx) = harness_with(cx, Some(session));
    cx.update(|window, cx| {
        select_line(&editor, Side::Right, "value = local;", window, cx);
        let take = editor
            .read(cx)
            .merge_line_take(MergeInput::Incoming)
            .unwrap();
        assert!(take.plan.baseline.is_empty());
        window.click("merge-selected-incoming", cx);
        assert_eq!(editor.read(cx).right.document.text(), "head\ntail\n");
        assert_eq!(
            editor
                .read(cx)
                .merge
                .as_ref()
                .unwrap()
                .session
                .unresolved()
                .count(),
            1
        );

        select_line(&editor, Side::Left, "value = local;", window, cx);
        let take = editor.read(cx).merge_line_take(MergeInput::Local).unwrap();
        assert!(take.plan.local.is_empty());
        window.click("merge-selected-local", cx);
        assert_eq!(
            editor.read(cx).right.document.text(),
            "head\nvalue = local;\ntail\n"
        );
        assert_eq!(
            editor
                .read(cx)
                .merge
                .as_ref()
                .unwrap()
                .session
                .unresolved()
                .count(),
            1
        );
        assert_synchronized(editor.read(cx));
    });
}

fn document(text: &str) -> Document {
    Document::from_bytes(text.as_bytes().to_vec()).unwrap()
}

#[gpui_kit::test]
fn selection_endpoints_and_ancestor_rows_never_broaden_a_take(cx: &mut TestAppContext) {
    let (editor, cx) = harness(cx);
    cx.update(|window, cx| {
        editor.update(cx, |view, cx| {
            view.toggle_merge_base(window, cx);
            let text = view.document(Side::Incoming).document.text();
            let start = text.find("    30").unwrap();
            let end = start + "    30\n".len();

            for (anchor, head) in [(start, end), (end, start)] {
                view.selection = Some(Selection {
                    side: Side::Incoming,
                    anchor,
                    head,
                });
                let take = view.merge_line_take(MergeInput::Incoming).unwrap();
                assert_eq!(view.right.document.copy_range(take.plan.local), "    45\n");
                assert_eq!(
                    view.document(Side::Incoming)
                        .document
                        .copy_range(take.plan.baseline),
                    "    30\n"
                );
            }

            view.selection = Some(Selection {
                side: Side::Incoming,
                anchor: 0,
                head: 0,
            });
            assert!(view.merge_line_take(MergeInput::Incoming).is_none());
            assert!(!view.merge_selection_active());
            view.selection.as_mut().unwrap().head = 3;
            assert!(view.merge_line_take(MergeInput::Incoming).is_none());
            assert!(view.merge_selection_active());
        });
    });
}

#[gpui_kit::test]
fn stale_line_preview_cannot_apply_after_a_same_length_edit(cx: &mut TestAppContext) {
    let (editor, cx) = harness(cx);
    cx.update(|window, cx| {
        select_line(&editor, Side::Incoming, "    30", window, cx);
        let before = editor
            .read(cx)
            .merge_line_take(MergeInput::Incoming)
            .unwrap();
        editor.update(cx, |view, cx| {
            let source_selection = view.selection.clone();
            let start = view.right.document.text().find("45").unwrap();
            view.selection = Some(Selection {
                side: Side::Right,
                anchor: start,
                head: start + 2,
            });
            view.replace_text_in_range(None, "99", window, cx);
            view.selection = source_selection;
            let fresh = view.merge_line_take(MergeInput::Incoming).unwrap();
            assert_eq!(fresh.plan, before.plan);

            view.apply_merge_line_take(&before, window, cx);
            assert!(view.right.document.text().contains("    99"));
            view.apply_merge_line_take(&fresh, window, cx);
            assert!(view.right.document.text().contains("    30"));
            assert_eq!(view.merge.as_ref().unwrap().session.unresolved().count(), 3);
        });
    });
}
