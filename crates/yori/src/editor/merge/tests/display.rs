//! Native selection-start and source-mapping behavior across classified rows.

use super::*;

#[gpui_kit::test]
fn chrome_rejects_selection_start_but_drag_crosses_chrome_and_eof(cx: &mut TestAppContext) {
    for base in ["base\n", ""] {
        let source = |text: &str| Document::from_bytes(text.as_bytes().to_vec()).unwrap();
        let session =
            MergeSession::new(source(base), source("local\n"), source("incoming\nextra\n"))
                .unwrap();
        let (editor, view) = harness_with(cx, Some(session));
        view.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.toggle_merge_base(window, cx);
                let display_len = editor.merge.as_ref().unwrap().display.rows().len();
                for side in [Side::Left, Side::Right, Side::Incoming] {
                    let point_for = |editor: &AlignedEditor, row| {
                        let origin = editor.content_bounds.get().origin;
                        point(
                            origin.x + px(editor.pane_left(side) + crate::editor::GUTTER_WIDTH),
                            origin.y
                                + px(crate::editor::HEADER_HEIGHT
                                    + display_units(row) * LINE_HEIGHT
                                    - editor.vertical_scroll
                                    + 1.0),
                        )
                    };
                    // Header, caption and either ancestor line or empty-ancestor row.
                    for row in 0..3 {
                        editor.selection = Some(Selection {
                            side,
                            anchor: 1,
                            head: 2,
                        });
                        let position = point_for(editor, row);
                        editor.mouse_down(
                            &gpui_kit::MouseDownEvent {
                                button: gpui_kit::MouseButton::Left,
                                position,
                                ..Default::default()
                            },
                            window,
                            cx,
                        );

                        let selected = editor.selection.as_ref().unwrap();
                        assert_eq!(
                            (selected.side, selected.anchor, selected.head),
                            (side, 1, 2)
                        );

                        editor.mouse_move(
                            &gpui_kit::MouseMoveEvent {
                                position,
                                pressed_button: Some(gpui_kit::MouseButton::Left),
                                ..Default::default()
                            },
                            window,
                            cx,
                        );
                        let selected = editor.selection.as_ref().unwrap();
                        assert_eq!((selected.anchor, selected.head), (1, 0));
                    }

                    // The final ordinary row has Local/Result gaps, not chrome.
                    editor.mouse_down(
                        &gpui_kit::MouseDownEvent {
                            button: gpui_kit::MouseButton::Left,
                            position: point_for(editor, display_len - 1),
                            ..Default::default()
                        },
                        window,
                        cx,
                    );
                    let expected = if side == Side::Incoming {
                        "incoming\n".len()
                    } else {
                        "local\n".len()
                    };
                    let selected = editor.selection.as_ref().unwrap();
                    assert_eq!((selected.anchor, selected.head), (expected, expected));

                    editor.mouse_move(
                        &gpui_kit::MouseMoveEvent {
                            position: point_for(editor, display_len + 2),
                            pressed_button: Some(gpui_kit::MouseButton::Left),
                            ..Default::default()
                        },
                        window,
                        cx,
                    );
                    let selected = editor.selection.as_ref().unwrap();
                    assert_eq!(selected.anchor, expected);
                    assert_eq!(selected.head, editor.document(side).document.text().len());
                }
                assert_synchronized(editor);
            });
        });
    }
}

#[gpui_kit::test]
fn empty_panes_keep_their_distinct_reverse_mapping_fallbacks(cx: &mut TestAppContext) {
    for (local, incoming, empty_side) in [
        ("", "incoming\n", Side::Left),
        ("local\n", "", Side::Incoming),
    ] {
        let source = |text: &str| Document::from_bytes(text.as_bytes().to_vec()).unwrap();
        let session = MergeSession::new(source("base\n"), source(local), source(incoming)).unwrap();
        let (editor, view) = harness_with(cx, Some(session));
        view.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.toggle_merge_base(window, cx);
                let display_len = editor.merge.as_ref().unwrap().display.rows().len();

                assert_eq!(
                    editor.row_for_source(empty_side, 0),
                    if empty_side == Side::Incoming {
                        display_len
                    } else {
                        0
                    }
                );
                for row in 0..display_len + 3 {
                    assert_eq!(editor.line_for_row(empty_side, row), None);
                    assert_eq!(editor.source_offset_for(empty_side, row, 10), 0);
                }
                if empty_side == Side::Left {
                    assert_eq!(editor.row_for_source(Side::Right, 0), 0);
                }
                assert_synchronized(editor);
            });
        });
    }
}
