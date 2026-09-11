use super::*;

struct Editor {
    vim: Vim,
    document: Document,
    history: EditHistory,
    selection: TextSelection,
    register: Register,
    writable: bool,
}

impl Editor {
    fn new(text: &str) -> Self {
        Self {
            vim: Vim::default(),
            document: Document::from_bytes(text.as_bytes().to_vec()).unwrap(),
            history: EditHistory::default(),
            selection: TextSelection::caret(0),
            register: Register::default(),
            writable: true,
        }
    }

    fn key(&mut self, key: &str) {
        let outcome = self
            .vim
            .handle(
                key,
                &mut self.document,
                &mut self.history,
                self.selection,
                &mut self.register,
                self.writable,
            )
            .unwrap();

        self.selection = outcome.selection;
        if !outcome.consumed {
            // The production Insert path uses this same document/history machinery.
            let edit = self
                .history
                .replace(
                    &mut self.document,
                    self.selection,
                    self.selection.range(),
                    key,
                )
                .unwrap();
            self.selection = edit.selection;
        }
    }

    fn keys(&mut self, keys: &str) {
        for key in keys.chars() {
            self.key(&key.to_string());
        }
    }

    fn text(&self) -> &str {
        self.document.text()
    }
}

#[test]
fn change_inner_word_and_insert_session_are_one_undo_step() {
    let mut editor = Editor::new("let old_name = 1;\r\n");
    editor.keys("wllciw");
    assert_eq!(editor.vim.mode(), Mode::Insert);
    assert_eq!(editor.text(), "let  = 1;\r\n");

    editor.keys("new_name");
    editor.key("escape");
    assert_eq!(editor.text(), "let new_name = 1;\r\n");

    editor.key("u");
    assert_eq!(editor.text(), "let old_name = 1;\r\n");

    editor.key("ctrl-r");
    assert_eq!(editor.text(), "let new_name = 1;\r\n");
}

#[test]
fn counted_delete_and_change_word_have_distinct_whitespace_semantics() {
    let mut editor = Editor::new("one two three four\ntail\n");
    editor.keys("3dw");
    assert_eq!(editor.text(), "four\ntail\n");

    editor.key("u");
    editor.keys("cw");
    editor.keys("1");
    editor.key("escape");
    assert_eq!(editor.text(), "1 two three four\ntail\n");

    editor.key("u");
    editor.keys("2d2w");
    assert_eq!(editor.text(), "\ntail\n");
}

#[test]
fn linewise_yank_paste_delete_and_undo_preserve_line_endings() {
    for (text, doubled) in [("a\r\nb\r\n", "a\r\na\r\nb\r\n"), ("a\nb", "a\na\nb")] {
        let mut editor = Editor::new(text);
        editor.keys("yyp");
        assert_eq!(editor.text(), doubled);

        editor.key("u");
        assert_eq!(editor.text(), text);

        editor.keys("Gdd");
        assert_eq!(
            editor.text(),
            if text.ends_with('\n') { "a\r\n" } else { "a" }
        );

        editor.key("u");
        assert_eq!(editor.text(), text);
    }
}

#[test]
fn visual_ranges_are_inclusive_and_linewise_ranges_cover_full_source_lines() {
    let mut editor = Editor::new("abc\ndef\nghi");
    editor.keys("vl");
    assert_eq!(editor.document.copy_range(editor.selection.range()), "ab");

    editor.key("d");
    assert_eq!(editor.text(), "c\ndef\nghi");

    editor.key("u");
    editor.keys("Vjy");
    assert_eq!(editor.register.text, "abc\ndef\n");

    editor.key("G");
    editor.key("p");
    assert_eq!(editor.text(), "abc\ndef\nghi\nabc\ndef");
}

#[test]
fn backwards_visual_selection_includes_both_endpoints() {
    let mut editor = Editor::new("abc\n");
    editor.keys("$vhy");
    assert_eq!(editor.register.text, "bc");
}

#[test]
fn grapheme_movement_and_deletion_never_split_unicode_or_crlf() {
    let mut editor = Editor::new("e\u{301}👩‍💻界\r\nlast");
    editor.key("l");
    assert_eq!(editor.selection.head, "e\u{301}".len());

    editor.key("x");
    assert_eq!(editor.text(), "e\u{301}界\r\nlast");

    editor.key("u");
    editor.keys("$x");
    assert_eq!(editor.text(), "e\u{301}👩‍💻\r\nlast");
}

#[test]
fn insert_commands_and_escape_keep_cursor_on_the_correct_line() {
    let cases = [
        ("i", "Xabc\n"),
        ("a", "aXbc\n"),
        ("I", "Xabc\n"),
        ("A", "abcX\n"),
        ("o", "abc\nX\n"),
        ("O", "X\nabc\n"),
    ];

    for (command, expected) in cases {
        let mut editor = Editor::new("abc\n");
        editor.key(command);
        editor.key("X");
        editor.key("escape");
        assert_eq!(editor.text(), expected, "{command}");

        editor.key("u");
        assert_eq!(editor.text(), "abc\n", "{command}");
    }
}

#[test]
fn empty_documents_blank_lines_and_unterminated_last_lines_are_editable() {
    for original in ["", "\n", "\r\n", "last"] {
        let mut editor = Editor::new(original);
        editor.keys("G$i");
        editor.key("X");
        editor.key("escape");
        editor.key("u");
        assert_eq!(editor.text(), original);
    }
}

#[test]
fn read_only_pane_allows_navigation_and_yank_but_no_mutation() {
    let mut editor = Editor::new("left\nright\n");
    editor.writable = false;

    editor.keys("jyy");
    assert_eq!(editor.register.text, "right\n");

    editor.keys("ddciwpioxu");
    editor.key("ctrl-r");
    assert_eq!(editor.text(), "left\nright\n");
    assert_eq!(editor.vim.mode(), Mode::Normal);
}

#[test]
fn cancelling_pending_commands_and_disabling_insert_preserves_history() {
    let mut editor = Editor::new("abc\n");
    editor.key("d");
    editor
        .vim
        .cancel(&editor.document, &mut editor.history, editor.selection);
    editor.key("w");
    assert_eq!(editor.text(), "abc\n");

    editor.key("i");
    editor.key("X");
    editor
        .vim
        .cancel(&editor.document, &mut editor.history, editor.selection);
    let edit = editor
        .history
        .replace(
            &mut editor.document,
            editor.selection,
            editor.selection.range(),
            "Y",
        )
        .unwrap();
    editor.selection = edit.selection;

    editor.key("u");
    assert_eq!(editor.text(), "abXc\n");

    editor.key("u");
    assert_eq!(editor.text(), "abc\n");
}

#[test]
fn mouse_selection_and_insert_repositioning_use_existing_history() {
    let mut editor = Editor::new("abc def\n");
    editor.selection = TextSelection { anchor: 0, head: 3 };
    editor.vim.select(&editor.document, editor.selection);

    editor.key("c");
    editor.key("X");
    editor
        .vim
        .reposition(&editor.document, &mut editor.history, editor.selection);
    assert_eq!(editor.vim.mode(), Mode::Insert);

    editor.selection = TextSelection::caret(0);
    editor
        .history
        .begin_transaction(&editor.document, editor.selection);
    editor.key("Y");
    editor.key("escape");
    assert_eq!(editor.text(), "YX def\n");

    editor.key("u");
    assert_eq!(editor.text(), "X def\n");

    editor.key("u");
    assert_eq!(editor.text(), "abc def\n");
}

#[test]
fn unnamed_register_can_cross_documents_with_different_line_endings() {
    let mut source = Editor::new("copied\r\n");
    source.keys("yy");

    let mut target = Editor::new("unterminated");
    target.register = source.register;
    target.key("p");
    assert_eq!(target.text(), "unterminated\ncopied");

    target.key("u");
    assert_eq!(target.text(), "unterminated");
}

#[test]
fn counted_inner_word_and_vertical_motions_respect_source_lines() {
    let mut editor = Editor::new("one two three\nx\nlonger line\n");
    editor.keys("c2iw");
    assert_eq!(editor.text(), " three\nx\nlonger line\n");

    editor.key("escape");
    editor.key("u");
    editor.keys("4ljj");
    assert_eq!(editor.selection.head, "one two three\nx\nlong".len());

    editor.keys("2gg");
    assert_eq!(editor.selection.head, "one two three\n".len());
}

#[test]
fn word_navigation_stops_at_blank_lines_and_dollar_preserves_end_column() {
    for newline in ["\n", "\r\n"] {
        let text = format!("first{newline}{newline}last longer{newline}");
        let mut editor = Editor::new(&text);
        editor.key("w");
        assert_eq!(editor.selection.head, "first".len() + newline.len());

        editor.key("w");
        assert_eq!(editor.selection.head, "first".len() + 2 * newline.len());

        editor.key("b");
        assert_eq!(editor.selection.head, "first".len() + newline.len());

        editor.keys("gg$jj");
        assert_eq!(editor.selection.head, text.len() - newline.len() - 1);
    }
}

#[test]
fn native_composition_and_typing_commit_as_one_modal_transaction() {
    let mut editor = Editor::new("old\r\n");
    editor.keys("ciw");

    for text in ["e", "e\u{301}"] {
        let range = editor
            .history
            .marked_range()
            .unwrap_or(editor.selection.range());
        let edit = editor
            .history
            .replace_marked(&mut editor.document, editor.selection, range, text, None)
            .unwrap();
        editor.selection = edit.selection;
    }
    editor
        .history
        .finish_composition(&editor.document, editor.selection);
    editor.key("!");
    editor.key("escape");
    assert_eq!(editor.text(), "e\u{301}!\r\n");

    editor.key("u");
    assert_eq!(editor.text(), "old\r\n");

    editor.key("ctrl-r");
    assert_eq!(editor.text(), "e\u{301}!\r\n");
}

#[test]
fn unknown_commands_do_not_insert_text_or_execute_a_partial_operator() {
    let mut editor = Editor::new("one two\n");
    editor.keys("dqwl");
    editor.keys("dcw");
    assert_eq!(editor.text(), "one two\n");

    editor.keys("gqi");
    assert_eq!(editor.vim.mode(), Mode::Insert);
}
