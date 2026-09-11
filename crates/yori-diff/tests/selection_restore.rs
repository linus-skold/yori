use yori_diff::{Alignment, SelectionRestore, restore_selection};
use yori_document::{
    Document,
    editing::{EditHistory, TextSelection},
};

fn doc(text: &str) -> Document {
    Document::from_bytes(text.as_bytes().to_vec()).unwrap()
}

fn plan(baseline: &Document, local: &Document, left_side: bool, text: &str) -> SelectionRestore {
    let selected = if left_side { baseline } else { local };
    let start = selected.text().find(text).unwrap();
    Alignment::between(baseline, local)
        .selection_restore(
            baseline,
            local,
            TextSelection {
                anchor: start,
                head: start + text.len(),
            },
            left_side,
        )
        .unwrap()
}

#[test]
fn selecting_part_of_a_line_on_either_side_splits_a_block_and_undoes_once() {
    let baseline = doc("start\nold one\nold two\nold three\nend\n");
    let original = "start\nnew one\nnew two\nnew three\nend\n";
    for left_side in [true, false] {
        let mut local = doc(original);
        let mut history = EditHistory::default();
        let selected = plan(&baseline, &local, left_side, "two");
        assert_eq!(selected.rows, 2..3);
        assert_eq!(baseline.copy_range(selected.baseline.clone()), "old two\n");
        assert_eq!(local.copy_range(selected.local.clone()), "new two\n");

        let before = TextSelection {
            anchor: selected.local.end,
            head: selected.local.start,
        };
        let edit =
            restore_selection(&mut history, &baseline, &mut local, before, &selected).unwrap();
        assert_eq!(local.text(), "start\nnew one\nold two\nnew three\nend\n");
        assert_eq!(Alignment::between(&baseline, &local).blocks().len(), 2);

        let undone = history.undo(&mut local, edit.selection).unwrap().unwrap();
        assert_eq!(local.text(), original);
        assert_eq!(undone.selection, before);
        assert!(history.undo(&mut local, before).unwrap().is_none());

        history.redo(&mut local, before).unwrap().unwrap();
        assert_eq!(local.text(), "start\nnew one\nold two\nnew three\nend\n");
    }
}

#[test]
fn selection_direction_and_line_start_endpoints_do_not_widen_the_restore() {
    let baseline = doc("a\nb\nc\n");
    let local = doc("A\nB\nC\n");
    let alignment = Alignment::between(&baseline, &local);
    for selection in [
        TextSelection { anchor: 2, head: 4 },
        TextSelection { anchor: 4, head: 2 },
    ] {
        let selected = alignment
            .selection_restore(&baseline, &local, selection, false)
            .unwrap();
        assert_eq!(selected.rows, 1..2);
        assert_eq!(selected.baseline, 2..4);
        assert_eq!(selected.local, 2..4);
    }
}

#[test]
fn added_and_removed_lines_restore_at_exact_gap_boundaries() {
    for (base, text, left_side, selected, expected) in [
        (
            "a\nz\n",
            "a\ncomment\nother\nz\n",
            false,
            "comment",
            "a\nother\nz\n",
        ),
        (
            "a\nmissing\nother\nz\n",
            "a\nz\n",
            true,
            "missing",
            "a\nmissing\nz\n",
        ),
        ("", "first\nsecond\n", false, "first", "second\n"),
        ("first\nsecond\n", "", true, "second", "second\n"),
        ("first\nend", "end", true, "first", "first\nend"),
        ("a\nlast", "a\n", true, "last", "a\nlast"),
    ] {
        let baseline = doc(base);
        let mut local = doc(text);
        let selected = plan(&baseline, &local, left_side, selected);
        let mut history = EditHistory::default();
        let edit = restore_selection(
            &mut history,
            &baseline,
            &mut local,
            TextSelection::caret(0),
            &selected,
        )
        .unwrap();
        assert_eq!(local.text(), expected);

        history.undo(&mut local, edit.selection).unwrap().unwrap();
        assert_eq!(local.text(), text);
    }
}

#[test]
fn selection_spanning_changes_preserves_equal_context_and_excludes_adjacent_gaps() {
    let baseline = doc("before\nold\nkeep\nmissing\nold2\nafter\n");
    let mut local = doc("before\nnew\nkeep\nnew2\nextra\nafter\n");
    let selected = plan(&baseline, &local, false, "new\nkeep\nnew2");
    let mut history = EditHistory::default();
    restore_selection(
        &mut history,
        &baseline,
        &mut local,
        TextSelection::caret(0),
        &selected,
    )
    .unwrap();
    // Pairing is positional within replacement runs: preview and application
    // agree on exactly the selected rows, not every row in a touched block.
    assert_eq!(local.text(), "before\nold\nkeep\nmissing\nextra\nafter\n");

    let baseline = doc("a\nremoved\nz\n");
    let local = doc("A\nz\n");
    assert_eq!(plan(&baseline, &local, false, "A").baseline, 0..2);
}

#[test]
fn opposite_side_gaps_inside_the_selection_participate_but_equal_edges_do_not() {
    for left_side in [true, false] {
        let baseline = doc("keep\nmissing\ntail\n");
        let mut local = doc("keep\ntail\n");
        let selected_text = if left_side {
            baseline.text()
        } else {
            local.text()
        }
        .to_owned();
        let selected = plan(&baseline, &local, left_side, &selected_text);
        assert_eq!(selected.rows, 1..2);
        assert_eq!(selected.baseline, 5..13);
        assert_eq!(selected.local, 5..5);

        restore_selection(
            &mut EditHistory::default(),
            &baseline,
            &mut local,
            TextSelection::caret(0),
            &selected,
        )
        .unwrap();
        assert_eq!(local.text(), baseline.text());
    }
}

#[test]
fn unicode_blank_lines_and_line_terminators_are_source_faithful() {
    for (base, text, selected, expected) in [
        ("α\r\n\r\nω", "β\r\nnote\r\nΩ", "note", "β\r\n\r\nΩ"),
        ("a\r\n", "A\n", "A", "a\r\n"),
        ("a", "A\n", "A", "a"),
        ("\n", "comment", "comment", "\n"),
        ("界\n", "👩‍💻\n", "👩‍💻", "界\n"),
        ("a", "A\nextra\n", "A", "a\nextra\n"),
        ("a\nlast", "a", "last", "a\nlast"),
    ] {
        let baseline = doc(base);
        let mut local = doc(text);
        let left_side = selected == "last";
        let selected = plan(&baseline, &local, left_side, selected);
        restore_selection(
            &mut EditHistory::default(),
            &baseline,
            &mut local,
            TextSelection::caret(0),
            &selected,
        )
        .unwrap();
        assert_eq!(local.text(), expected);
    }
}

#[test]
fn carets_equal_lines_and_invalid_selections_offer_no_restore() {
    let baseline = doc("same\nα\n");
    let local = doc("same\nβ\n");
    let alignment = Alignment::between(&baseline, &local);
    for selection in [
        TextSelection::caret(6),
        TextSelection { anchor: 0, head: 5 },
        TextSelection {
            anchor: 0,
            head: 99,
        },
        TextSelection { anchor: 5, head: 6 },
    ] {
        assert!(
            alignment
                .selection_restore(&baseline, &local, selection, false)
                .is_none()
        );
    }
}
