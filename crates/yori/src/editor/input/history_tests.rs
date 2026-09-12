//! History routing uses the focused comparison, not the selected document's mutability.

use super::*;
use crate::editor::PaneDocument;
use gpui_kit::component::Root;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{AppContext, Entity, TestAppContext, VisualTestContext};
use yori_document::Document;

fn harness(
    cx: &mut TestAppContext,
    merging: bool,
) -> (Entity<AlignedEditor>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::appearance::init(cx);
        crate::editor::init(cx);
    });

    let mut editor = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            if merging {
                AlignedEditor::merge_fixture(window, cx)
            } else {
                let pane = |text: &str| {
                    PaneDocument::new(
                        "fixture.txt".into(),
                        Document::from_bytes(text.as_bytes().to_vec()).unwrap(),
                    )
                };
                AlignedEditor::new(pane("baseline\n"), pane("local\n"), window, cx)
            }
        });
        editor = Some(view.clone());

        Root::new(view, window, cx)
    });
    cx.update(TestWindowExt::render_frame);
    cx.run_until_parked();

    (editor.unwrap(), cx)
}

fn edit_result(editor: &Entity<AlignedEditor>, window: &mut Window, cx: &mut App) -> String {
    let original = editor.read(cx).right.document.text().to_owned();
    editor.update(cx, |editor, cx| {
        editor.selection = Some(Selection {
            side: Side::Right,
            anchor: 0,
            head: 0,
        });
        editor.focus.focus(window, cx);
        cx.notify();
    });
    window.render_frame(cx);
    window.input("X", cx);
    assert_eq!(
        editor.read(cx).right.document.text(),
        format!("X{original}")
    );

    original
}

fn check_input_history(cx: &mut TestAppContext, merging: bool, vim: bool) {
    let (editor, cx) = harness(cx, merging);
    cx.update(|window, cx| {
        let original = edit_result(&editor, window, cx);
        if vim {
            editor.update(cx, |editor, cx| editor.toggle_vim(true, window, cx));
        }

        let sides = if merging {
            vec![Side::Left, Side::Incoming]
        } else {
            vec![Side::Left]
        };
        for side in sides {
            let x = editor.read(cx).pane_left(side) + GUTTER_WIDTH + 2.0;
            window.click_at("rows-viewport", point(px(x), px(11.0)), cx);
            window.press("ctrl-a", cx);
            let selected = editor.read(cx).selection.clone().unwrap();
            let source = editor.read(cx).document(side).document.text().to_owned();
            assert_eq!(selected.side, side);

            window.press(if vim { "u" } else { "ctrl-z" }, cx);
            assert_eq!(editor.read(cx).right.document.text(), original);
            let after = editor.read(cx).selection.as_ref().unwrap();
            assert_eq!(
                (after.side, after.anchor, after.head),
                (side, selected.anchor, selected.head)
            );
            assert!(editor.read(cx).focus.is_focused(window));
            window.press("ctrl-c", cx);
            assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), source);

            window.press(if vim { "ctrl-r" } else { "ctrl-shift-z" }, cx);
            assert_eq!(
                editor.read(cx).right.document.text(),
                format!("X{original}")
            );
            let after = editor.read(cx).selection.as_ref().unwrap();
            assert_eq!(
                (after.side, after.anchor, after.head),
                (side, selected.anchor, selected.head)
            );
            assert_eq!(editor.read(cx).document(side).document.text(), source);
        }
    });
}

#[gpui_kit::test]
fn diff_history_works_from_baseline_without_moving_its_selection(cx: &mut TestAppContext) {
    check_input_history(cx, false, false);
}

#[gpui_kit::test]
fn merge_history_works_from_both_inputs_without_moving_their_selection(cx: &mut TestAppContext) {
    check_input_history(cx, true, false);
}

#[gpui_kit::test]
fn vim_diff_history_works_from_baseline(cx: &mut TestAppContext) {
    check_input_history(cx, false, true);
}

#[gpui_kit::test]
fn vim_merge_history_works_from_both_inputs(cx: &mut TestAppContext) {
    check_input_history(cx, true, true);
}

#[gpui_kit::test]
fn resolution_decisions_undo_from_incoming_without_making_it_editable(cx: &mut TestAppContext) {
    let (editor, cx) = harness(cx, true);
    cx.update(|window, cx| {
        let original = editor.read(cx).right.document.text().to_owned();
        window.click(("merge-incoming-button", 0usize), cx);
        let accepted = editor.read(cx).right.document.text().to_owned();
        let x = editor.read(cx).pane_left(Side::Incoming) + GUTTER_WIDTH + 2.0;
        window.click_at("rows-viewport", point(px(x), px(11.0)), cx);

        window.press("ctrl-z", cx);
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
        assert_eq!(
            editor.read(cx).selection.as_ref().unwrap().side,
            Side::Incoming
        );
        window.input("Q", cx);
        assert_eq!(editor.read(cx).right.document.text(), original);

        window.press("ctrl-shift-z", cx);
        assert_eq!(editor.read(cx).right.document.text(), accepted);
        assert_eq!(
            editor
                .read(cx)
                .merge
                .as_ref()
                .unwrap()
                .session
                .unresolved()
                .count(),
            2
        );
        assert_eq!(
            editor.read(cx).selection.as_ref().unwrap().side,
            Side::Incoming
        );
    });
}

#[gpui_kit::test]
fn another_focus_cannot_borrow_the_comparisons_history(cx: &mut TestAppContext) {
    let (editor, cx) = harness(cx, true);
    cx.update(|window, cx| {
        let original = edit_result(&editor, window, cx);
        let other_focus = cx.focus_handle();
        other_focus.focus(window, cx);

        // Even an action bubbling from another control cannot consume result history.
        editor.update(cx, |editor, cx| editor.travel_history(false, window, cx));
        assert_eq!(
            editor.read(cx).right.document.text(),
            format!("X{original}")
        );
        assert!(other_focus.is_focused(window));

        editor.read(cx).focus.clone().focus(window, cx);
        window.press("ctrl-z", cx);
        assert_eq!(editor.read(cx).right.document.text(), original);
    });
}
