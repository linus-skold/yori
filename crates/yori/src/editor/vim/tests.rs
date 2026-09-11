//! Behavioral routing tests: real GPUI key dispatch, native typing, and history.

use super::*;
use crate::editor::PaneDocument;
use gpui_kit::component::Root;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{AppContext, Entity, TestAppContext, VisualTestContext};
use yori_document::Document;

fn harness(cx: &mut TestAppContext) -> (Entity<AlignedEditor>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::appearance::init(cx);
        crate::editor::init(cx);
    });

    let mut editor = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let pane = |text: &str| {
            PaneDocument::new(
                "fixture.txt".into(),
                Document::from_bytes(text.as_bytes().to_vec()).unwrap(),
            )
        };
        let view =
            cx.new(|cx| AlignedEditor::new(pane("baseline\n"), pane("let old = 1;\n"), window, cx));
        editor = Some(view.clone());

        Root::new(view, window, cx)
    });

    cx.update(TestWindowExt::render_frame);
    cx.run_until_parked();

    (editor.unwrap(), cx)
}

#[gpui_kit::test]
fn modal_commands_route_before_native_text_and_undo_the_whole_change(cx: &mut TestAppContext) {
    let (editor, cx) = harness(cx);
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.toggle_vim(true, window, cx));
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        for key in ["w", "c", "i", "w"] {
            window.press(key, cx);
        }

        assert_eq!(editor.read(cx).right.document.text(), "let  = 1;\n");
        assert_eq!(editor.read(cx).vim.mode(), Mode::Insert);

        for key in ["n", "e", "w", "escape"] {
            window.press(key, cx);
        }

        assert_eq!(editor.read(cx).right.document.text(), "let new = 1;\n");
        assert_eq!(editor.read(cx).vim.mode(), Mode::Normal);

        window.press("u", cx);
        assert_eq!(editor.read(cx).right.document.text(), "let old = 1;\n");

        window.press("ctrl-r", cx);
        assert_eq!(editor.read(cx).right.document.text(), "let new = 1;\n");
    });
}

fn toggle_vim_from_options(window: &mut Window, cx: &mut App) {
    window.click("editor-options", cx);
    window.render_frame(cx);

    window.press("down", cx);
    window.press("down", cx);
    window.press("down", cx);
    window.press("enter", cx);
}

#[gpui_kit::test]
fn toggling_off_restores_conventional_typing_without_losing_undo(cx: &mut TestAppContext) {
    let (editor, cx) = harness(cx);
    cx.update(toggle_vim_from_options);
    cx.run_until_parked();
    cx.update(|window, cx| {
        for key in ["shift-a", "x", "y"] {
            window.press(key, cx);
        }

        assert_eq!(editor.read(cx).right.document.text(), "let old = 1;xy\n");

        toggle_vim_from_options(window, cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.press("z", cx);
        assert_eq!(editor.read(cx).right.document.text(), "let old = 1;xyz\n");

        window.press("ctrl-z", cx);
        assert_eq!(editor.read(cx).right.document.text(), "let old = 1;xy\n");

        window.press("ctrl-z", cx);
        assert_eq!(editor.read(cx).right.document.text(), "let old = 1;\n");
    });
}

#[gpui_kit::test]
fn baseline_is_navigable_and_copyable_but_cannot_enter_insert(cx: &mut TestAppContext) {
    let (editor, cx) = harness(cx);
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.toggle_vim(true, window, cx);
            editor.selection = Some(Selection {
                side: Side::Left,
                anchor: 0,
                head: 0,
            });
        });
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        for key in ["l", "y", "y", "d", "d", "i", "x"] {
            window.press(key, cx);
        }

        assert_eq!(editor.read(cx).left.document.text(), "baseline\n");
        assert_eq!(editor.read(cx).right.document.text(), "let old = 1;\n");
        assert_eq!(editor.read(cx).vim.mode(), Mode::Normal);
        assert_eq!(editor.read(cx).selection.as_ref().unwrap().side, Side::Left);
    });
}
