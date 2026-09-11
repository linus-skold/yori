use std::fmt::Write as _;
use yori::{
    display::{DisplayLine, max_display_columns, source_offset_at},
    geometry::{EditorGeometry, LocalHit, display_units, horizontal_scroll_limit},
};
use yori_diff::{Alignment, DiffKind};
use yori_document::{Document, InputError, LineEnding};

fn doc(text: &str) -> Document {
    Document::from_bytes(text.as_bytes().to_vec()).unwrap()
}

fn rows(left: &str, right: &str) -> Vec<(Option<usize>, Option<usize>, DiffKind)> {
    Alignment::between(&doc(left), &doc(right))
        .rows()
        .iter()
        .map(|row| (row.left, row.right, row.kind))
        .collect()
}

#[test]
fn splitting_preserves_line_endings_and_final_newline_state() {
    let source = doc("one\r\n\r\nthree\nlast");
    assert_eq!(source.lines().len(), 4);
    assert_eq!(source.content(0), "one");
    assert_eq!(source.full_line(0), "one\r\n");
    assert_eq!(source.lines()[0].ending, LineEnding::CrLf);
    assert_eq!(source.content(1), "");
    assert_eq!(source.full_line(1), "\r\n");
    assert_eq!(source.lines()[2].ending, LineEnding::Lf);
    assert_eq!(source.lines()[3].ending, LineEnding::None);
    assert!(doc("").lines().is_empty());
    assert_eq!(doc("\n").lines().len(), 1);
}

#[test]
fn unsupported_bytes_are_rejected_instead_of_transformed() {
    assert!(matches!(
        Document::from_bytes(vec![0xff]),
        Err(InputError::InvalidUtf8)
    ));
    assert!(matches!(
        Document::from_bytes(b"a\0b".to_vec()),
        Err(InputError::ContainsNul)
    ));
    assert!(matches!(
        Document::from_bytes(b"a\rb".to_vec()),
        Err(InputError::BareCarriageReturn { offset: 1 })
    ));
}

#[test]
fn aligns_equal_modified_and_inserted_or_deleted_boundaries() {
    assert_eq!(
        rows("a\nb\n", "a\nb\n"),
        vec![
            (Some(0), Some(0), DiffKind::Equal),
            (Some(1), Some(1), DiffKind::Equal)
        ]
    );
    assert_eq!(
        rows("a\n", "b\n"),
        vec![(Some(0), Some(0), DiffKind::Modified)]
    );
    assert_eq!(
        rows("b\n", "a\nb\nc\n"),
        vec![
            (None, Some(0), DiffKind::Added),
            (Some(0), Some(1), DiffKind::Equal),
            (None, Some(2), DiffKind::Added)
        ]
    );
    assert_eq!(
        rows("a\nb\nc\n", "b\n"),
        vec![
            (Some(0), None, DiffKind::Removed),
            (Some(1), Some(0), DiffKind::Equal),
            (Some(2), None, DiffKind::Removed)
        ]
    );
}

#[test]
fn handles_middle_changes_empty_sides_real_blank_lines_and_final_newlines() {
    assert_eq!(rows("a\nb\nc\n", "a\nx\nc\n")[1].2, DiffKind::Modified);
    assert_eq!(rows("", "x\n"), vec![(None, Some(0), DiffKind::Added)]);
    assert_eq!(rows("x\n", ""), vec![(Some(0), None, DiffKind::Removed)]);
    assert_eq!(
        rows("a\n\nb\n", "a\nb\n")[1],
        (Some(1), None, DiffKind::Removed)
    );
    assert_eq!(rows("a\n", "a")[0].2, DiffKind::Modified);
    assert_eq!(rows("a\r\n", "a\n")[0].2, DiffKind::Modified);
}

#[test]
fn copy_uses_original_source_bytes_across_alignment_gaps() {
    let left = doc("zero\r\none\r\ntwo");
    let right = doc("zero\r\ninserted\r\none\r\ntwo");
    let alignment = Alignment::between(&left, &right);
    let gap = alignment
        .rows()
        .iter()
        .position(|row| row.left.is_none())
        .unwrap();
    let boundary = alignment.gap_offset(&left, gap, true);
    assert_eq!(boundary, "zero\r\n".len());
    assert_eq!(left.copy_range(0..left.text().len()), "zero\r\none\r\ntwo");
    assert!(!left.copy_range(0..left.text().len()).contains("inserted"));
}

#[test]
fn leading_trailing_and_all_gap_rows_have_stable_source_affinity() {
    let empty = doc("");
    let content = doc("a\nb\n");
    let alignment = Alignment::between(&empty, &content);
    assert_eq!(alignment.gap_offset(&empty, 0, true), 0);
    assert_eq!(alignment.gap_offset(&empty, 1, true), 0);

    let left = doc("b\n");
    let right = doc("a\nb\nc\n");
    let alignment = Alignment::between(&left, &right);
    assert_eq!(alignment.gap_offset(&left, 0, true), 0);
    assert_eq!(alignment.gap_offset(&left, 2, true), left.text().len());
}

#[test]
fn unicode_and_tabs_map_shaped_display_boundaries_to_source_bytes() {
    let display = DisplayLine::from_source("α\t界", 10, 4);
    assert_eq!(display.text, "α   界");
    assert_eq!(display.columns(), 6);
    assert_eq!(display.source_offset(0), 10);
    assert_eq!(display.source_offset("α".len()), 12);
    assert_eq!(display.source_offset(display.text.len()), 16);
    assert_eq!(display.display_range(12..13), 2..5);
    assert_eq!(display.display_range(13..16), 5..8);
    for (display_byte, _) in display.text.char_indices() {
        assert!("α\t界".is_char_boundary(display.source_offset(display_byte) - 10));
    }
}

#[test]
fn row_below_document_reaches_original_eof_terminators() {
    for source in ["a\n", "a\r\n", "\n", "a"] {
        let document = doc(source);
        let alignment = Alignment::between(&document, &document);
        let last_row = alignment.rows().len().saturating_sub(1);
        let line_end = source_offset_at(&alignment, &document, last_row, true, usize::MAX, 4);
        assert_eq!(
            line_end,
            document.lines().last().map_or(0, |line| line.content.end)
        );

        let eof = source_offset_at(&alignment, &document, alignment.rows().len(), true, 0, 4);
        assert_eq!(eof, source.len());
        assert_eq!(document.copy_range(0..eof), source);
    }
}

#[test]
fn tab_expanded_horizontal_extent_can_reveal_the_final_source_position() {
    let source = format!("{}tail\n", "\t".repeat(256));
    let document = doc(&source);
    let columns = max_display_columns(&document, 4);
    assert_eq!(columns, 1_028);

    let cell_width = 8.0;
    let viewport_width = 640.0;
    let limit = horizontal_scroll_limit(columns, cell_width, viewport_width);
    let tail_after_full_scroll = display_units(columns) * cell_width - limit;
    assert!(tail_after_full_scroll <= viewport_width);
    assert!(limit > display_units(document.lines()[0].content.len()) * 12.0);
}

#[test]
fn local_editor_geometry_is_independent_of_desktop_placement() {
    let geometry = EditorGeometry::new(21.0, 17.0, 1_000.0, 700.0, 42.0, 58.0, 20.0);
    let content_local_pointer = (750.0, 91.0);
    let window_local_pointer = (
        content_local_pointer.0 + 21.0,
        content_local_pointer.1 + 17.0,
    );
    let expected = LocalHit {
        left_side: false,
        row: 3,
        in_gutter: false,
        text_x: 229.0,
    };
    assert_eq!(
        geometry.hit(window_local_pointer.0, window_local_pointer.1, 11.0, 37.0),
        expected
    );

    let document = doc("aa\nbb\ncc\ndd\n");
    let alignment = Alignment::between(&document, &document);
    for desktop_origin in [(0.0, 0.0), (500.0, 300.0), (-800.0, 120.0)] {
        let global_pointer = (
            window_local_pointer.0 + desktop_origin.0,
            window_local_pointer.1 + desktop_origin.1,
        );
        let event_position = (
            global_pointer.0 - desktop_origin.0,
            global_pointer.1 - desktop_origin.1,
        );
        let hit = geometry.hit(event_position.0, event_position.1, 11.0, 37.0);
        assert_eq!(hit, expected);
        assert_eq!(
            source_offset_at(&alignment, &document, hit.row, hit.left_side, 2, 4),
            11
        );
    }
}

#[test]
#[expect(
    clippy::float_cmp,
    reason = "these whole/half pixel fixtures are exactly representable; clipping must match the exact geometry"
)]
fn geometry_keeps_gutter_and_header_outside_scrolled_content() {
    let geometry = EditorGeometry::new(21.0, 17.0, 1_000.0, 700.0, 42.0, 58.0, 20.0);
    assert_eq!(geometry.pane_width(), 500.0);
    assert_eq!(geometry.text_viewport_width(), 442.0);
    assert_eq!(geometry.rows_viewport_height(), 658.0);

    let left_gutter = geometry.hit(61.0, 69.0, 10.0, 80.0);
    assert!(left_gutter.left_side);
    assert_eq!(left_gutter.row, 1);
    assert!(left_gutter.in_gutter);
    assert_eq!(left_gutter.text_x, 0.0);

    let right_text_start = geometry.hit(579.0, 59.0, 0.0, 0.0);
    assert!(!right_text_start.left_side);
    assert_eq!(right_text_start.row, 0);
    assert!(!right_text_start.in_gutter);
    assert_eq!(right_text_start.text_x, 0.0);

    assert_eq!(geometry.visible_row_top(4, 4, 7.5), -7.5);
    assert_eq!(geometry.visible_row_top(5, 4, 7.5), 12.5);
    assert_eq!(geometry.vertical_scroll_limit(100), 1_362.0);
}

#[test]
#[ignore = "explicit 100k-line stress measurement"]
fn stress_mapping_handles_one_hundred_thousand_lines() {
    let started = std::time::Instant::now();
    let mut left = String::new();
    let mut right = String::new();
    for line in 0..100_000 {
        writeln!(left, "fn row_{line}() -> usize {{ {line} }}").unwrap();
        if line % 997 == 0 {
            right.push_str("// inserted 👋\n");
        }
        let value = if line % 503 == 0 { line + 1 } else { line };
        writeln!(right, "fn row_{line}() -> usize {{ {value} }}").unwrap();
    }
    let left = doc(&left);
    let right = doc(&right);
    let max_columns = max_display_columns(&left, 4).max(max_display_columns(&right, 4));
    let alignment = Alignment::between(&left, &right);
    assert!(max_columns >= 30);
    assert!(alignment.rows().len() >= 100_000);
    assert_eq!(left.copy_range(0..left.text().len()), left.text());
    eprintln!(
        "100k split/extent/alignment/source-copy check: {:?}, {} alignment rows, {max_columns} display columns",
        started.elapsed(),
        alignment.rows().len()
    );
}
