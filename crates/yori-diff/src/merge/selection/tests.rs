use super::*;
use crate::{
    Alignment,
    merge::{ConflictId, Take},
};
use yori_document::Document;

fn document(text: &str) -> Document {
    Document::from_bytes(text.as_bytes().to_vec()).unwrap()
}

fn plan(
    merge: &MergeSession,
    input: MergeInput,
    selected: &str,
    in_result: bool,
) -> SelectionRestore {
    let source = match input {
        MergeInput::Local => merge.local(),
        MergeInput::Incoming => merge.incoming(),
    };
    let rows = merge.alignment();
    let alignment = Alignment::from_projection(
        source,
        merge.result(),
        rows.rows().iter().map(|row| {
            (
                match input {
                    MergeInput::Local => row.local,
                    MergeInput::Incoming => row.incoming,
                },
                row.result,
            )
        }),
    );
    let document = if in_result { merge.result() } else { source };
    let start = document.text().find(selected).unwrap();

    alignment
        .selection_restore(
            source,
            merge.result(),
            TextSelection {
                anchor: start,
                head: start + selected.len(),
            },
            !in_result,
        )
        .unwrap()
}

#[test]
fn selected_takes_keep_all_resolution_states_and_share_undo() {
    for input in [MergeInput::Local, MergeInput::Incoming] {
        for in_result in [false, true] {
            let mut merge = MergeSession::new(
                document("head\nfirst = base;\nkeep\nsecond = base;\ntail\n"),
                document("head\nfirst = local;\nkeep\nsecond = local;\ntail\n"),
                document("head\nfirst = incoming;\nkeep\nsecond = incoming;\ntail\n"),
            )
            .unwrap();
            if input == MergeInput::Local {
                merge
                    .take(ConflictId(0), Take::Incoming, TextSelection::caret(0))
                    .unwrap();
            } else {
                merge
                    .set_resolved(ConflictId(0), true, TextSelection::caret(0))
                    .unwrap();
            }
            let before = merge.result().text().to_owned();
            let states: Vec<_> = merge
                .conflicts()
                .iter()
                .map(|conflict| merge.state(conflict.id).unwrap().clone())
                .collect();
            let selected = plan(&merge, input, "first =", in_result);
            let source = if input == MergeInput::Local {
                merge.local()
            } else {
                merge.incoming()
            };
            let expected = format!(
                "{}{}{}",
                &before[..selected.local.start],
                source.copy_range(selected.baseline.clone()),
                &before[selected.local.end..]
            );

            merge
                .take_lines(input, TextSelection::caret(selected.local.start), &selected)
                .unwrap();
            assert_eq!(merge.result().text(), expected);
            assert!(merge.state(ConflictId(0)).unwrap().resolved);
            assert!(!merge.state(ConflictId(1)).unwrap().resolved);

            merge.undo(TextSelection::caret(0)).unwrap().unwrap();
            assert_eq!(merge.result().text(), before);
            for (conflict, state) in merge.conflicts().iter().zip(states) {
                assert_eq!(merge.state(conflict.id).unwrap(), &state);
            }
            merge.redo(TextSelection::caret(0)).unwrap().unwrap();
            assert_eq!(merge.result().text(), expected);
        }
    }
}

#[test]
fn taking_across_conflicts_preserves_their_independent_reset_ranges() {
    let mut merge = MergeSession::new(
        document("head\nfirst = base;\nkeep\nsecond = base;\ntail\n"),
        document("head\nfirst = local;\nkeep\nsecond = local;\ntail\n"),
        document("head\nfirst = incoming;\nkeep\nsecond = incoming;\ntail\n"),
    )
    .unwrap();
    let selected = plan(&merge, MergeInput::Incoming, merge.incoming().text(), false);
    merge
        .take_lines(MergeInput::Incoming, TextSelection::caret(0), &selected)
        .unwrap();
    assert_eq!(merge.result().text(), merge.incoming().text());
    assert_eq!(merge.unresolved().count(), 2);

    merge.reset(ConflictId(0), TextSelection::caret(0)).unwrap();
    assert_eq!(
        merge.result().text(),
        "head\nfirst = local;\nkeep\nsecond = incoming;\ntail\n"
    );
    merge.reset(ConflictId(1), TextSelection::caret(0)).unwrap();
    assert_eq!(merge.result().text(), merge.local().text());
}

#[test]
fn line_takes_insert_into_gaps_and_delete_result_only_lines() {
    for deleting in [false, true] {
        let (local, incoming) = if deleting {
            ("head\nvalue = local;\ntail\n", "head\ntail\n")
        } else {
            ("head\ntail\n", "head\nvalue = incoming;\ntail\n")
        };
        let mut merge = MergeSession::new(
            document("head\nvalue = base;\ntail\n"),
            document(local),
            document(incoming),
        )
        .unwrap();
        let selected = plan(&merge, MergeInput::Incoming, "value =", deleting);
        assert_eq!(selected.baseline.is_empty(), deleting);
        assert_eq!(selected.local.is_empty(), !deleting);

        merge
            .take_lines(
                MergeInput::Incoming,
                TextSelection::caret(selected.local.start),
                &selected,
            )
            .unwrap();
        assert_eq!(merge.result().text(), incoming);
        assert_eq!(merge.unresolved().count(), 1);

        merge.undo(TextSelection::caret(0)).unwrap().unwrap();
        assert_eq!(merge.result().text(), local);
    }
}

#[test]
fn unicode_crlf_and_unterminated_fragments_preserve_source_boundaries() {
    let mut merge = MergeSession::new(
        document("head\r\nvalue = base;\r\ntail"),
        document("head\r\nvalue = local;\r\ntail"),
        document("head\r\nvalue = 日本語;\r\ntail"),
    )
    .unwrap();
    let selected = plan(&merge, MergeInput::Incoming, "日本語", false);
    merge
        .take_lines(MergeInput::Incoming, TextSelection::caret(0), &selected)
        .unwrap();
    assert_eq!(merge.result().text(), "head\r\nvalue = 日本語;\r\ntail");

    let mut merge = MergeSession::new(
        document("value = base;\n"),
        document("value = local;\n"),
        document("value = incoming;"),
    )
    .unwrap();
    let end = merge.result().text().len();
    merge
        .replace(TextSelection::caret(end), end..end, "tail\n")
        .unwrap();
    let selected = plan(&merge, MergeInput::Incoming, "value =", false);
    merge
        .take_lines(MergeInput::Incoming, TextSelection::caret(0), &selected)
        .unwrap();
    assert_eq!(merge.result().text(), "value = incoming;\ntail\n");
}
