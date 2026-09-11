//! Input-level regressions on GPUI's headless test platform; no pixel captures.

use super::*;
use gpui_kit::component::Root;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{TestAppContext, VisualTestContext, point};
use std::path::Path;

fn harness(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::appearance::init(cx);
        crate::editor::init(cx);
        super::init(cx);
    });
    let mut workspace = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| Workspace::new(window, cx));
        workspace = Some(view.clone());
        Root::new(view, window, cx)
    });
    let workspace = workspace.unwrap();
    cx.update(|window, cx| {
        workspace.update(cx, |view, cx| {
            let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
            view.open_paths(
                &fixtures.join("before.rs"),
                &fixtures.join("after.rs"),
                window,
                cx,
            );
        });
        window.render_frame(cx);
    });
    cx.run_until_parked();

    (workspace, cx)
}

#[gpui_kit::test]
fn startup_and_handoff_dispatch_can_read_modal_state_and_report_file_errors(
    cx: &mut TestAppContext,
) {
    let (workspace, cx) = harness(cx);
    let window = cx.update(|window, _| window.window_handle().downcast::<Root>().unwrap());
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");

    crate::dispatch_open(window, &workspace, &[], &mut cx.cx).unwrap();
    crate::dispatch_open(
        window,
        &workspace,
        &[(
            fixtures.join("intraline-before.rs"),
            fixtures.join("intraline-after.rs"),
        )],
        &mut cx.cx,
    )
    .unwrap();
    cx.update(|_, cx| assert_eq!(workspace.read(cx).tabs.entries.len(), 2));

    let error = crate::dispatch_open(
        window,
        &workspace,
        &[(fixtures.join("missing.rs"), fixtures.join("after.rs"))],
        &mut cx.cx,
    )
    .unwrap_err();
    assert!(error.contains("missing.rs"));
    cx.update(|_, cx| assert_eq!(workspace.read(cx).tabs.entries.len(), 2));
}

#[gpui_kit::test]
fn switching_tabs_uses_current_geometry_on_the_first_frame(cx: &mut TestAppContext) {
    let (workspace, cx) = harness(cx);
    cx.update(|window, cx| {
        workspace.update(cx, |view, cx| {
            let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
            view.open_paths(
                &fixtures.join("intraline-before.rs"),
                &fixtures.join("intraline-after.rs"),
                window,
                cx,
            );
        });
        window.render_frame(cx);
    });
    cx.run_until_parked();

    // The hidden editor retains its old measurement while the window changes.
    cx.simulate_resize(gpui_kit::size(px(900.0), px(700.0)));
    cx.run_until_parked();

    cx.update(|window, cx| {
        workspace.update(cx, |view, cx| view.activate(0, window, cx));
        window.render_frame(cx);

        let editor = window.find("aligned-editor").bounds();
        let rows = window.find("rows-viewport").bounds();
        assert_eq!(
            rows.size.width, editor.size.width,
            "pane widths must be correct immediately after activation"
        );
        assert_eq!(
            rows.bottom(),
            editor.bottom(),
            "rows must reach the current viewport bottom on the first frame"
        );

        window.render_frame(cx);
        assert_eq!(
            window.find("rows-viewport").bounds(),
            rows,
            "a later frame must not reposition the panes"
        );
    });
}

#[gpui_kit::test]
fn close_button_removes_a_clean_comparison(cx: &mut TestAppContext) {
    let (workspace, cx) = harness(cx);

    cx.update(|window, cx| window.click(("tab-close-target", 0usize), cx));
    cx.run_until_parked();

    cx.update(|_, cx| {
        assert!(
            workspace.read(cx).tabs.entries.is_empty(),
            "clicking close must remove the clean tab"
        );
    });
}

#[gpui_kit::test]
fn native_close_accepts_a_clean_window(cx: &mut TestAppContext) {
    let (_, cx) = harness(cx);

    assert!(
        cx.simulate_close(),
        "the compositor close request must be accepted for a clean window"
    );
}

fn copy_active_text(window: &mut Window, cx: &mut App) -> String {
    let command = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    window.press(&format!("{command}-a"), cx);
    window.press(&format!("{command}-c"), cx);

    cx.read_from_clipboard()
        .and_then(|item| item.text())
        .expect("the editor should copy its selection")
}

#[gpui_kit::test]
fn closing_a_modified_tab_can_keep_then_discard_its_edits(cx: &mut TestAppContext) {
    let (workspace, cx) = harness(cx);
    let edited_text = cx.update(|window, cx| {
        let width = window.find("rows-viewport").bounds().size.width;
        window.click_at("rows-viewport", point(width * 0.75, px(11.0)), cx);
        window.input("changed", cx);
        copy_active_text(window, cx)
    });
    cx.run_until_parked();

    cx.update(|window, cx| {
        assert!(workspace.read(cx).has_modified_tabs(cx));
        window.click(("tab-close-target", 0usize), cx);
    });
    cx.run_until_parked();

    cx.update(|window, cx| {
        window.render_frame(cx);
        assert_eq!(workspace.read(cx).tabs.entries.len(), 1);
        window.click("cancel", cx);
    });
    cx.run_until_parked();

    cx.update(|window, cx| {
        assert!(!window.has_active_dialog(cx));
        assert_eq!(workspace.read(cx).tabs.entries.len(), 1);
        assert!(workspace.read(cx).has_modified_tabs(cx));
        assert_eq!(copy_active_text(window, cx), edited_text);

        window.click(("tab-close-target", 0usize), cx);
        window.click("ok", cx);
    });
    cx.run_until_parked();

    cx.update(|window, cx| {
        assert!(!window.has_active_dialog(cx));
        assert!(workspace.read(cx).tabs.entries.is_empty());
    });
}

#[gpui_kit::test]
fn forwarded_pairs_preserve_existing_edits_and_load_new_files_before_returning(
    cx: &mut TestAppContext,
) {
    let (workspace, cx) = harness(cx);
    let edited_text = cx.update(|window, cx| {
        let width = window.find("rows-viewport").bounds().size.width;
        window.click_at("rows-viewport", point(width * 0.75, px(11.0)), cx);
        window.input("changed", cx);
        copy_active_text(window, cx)
    });
    let temporary = tempfile::tempdir().unwrap();
    let left = temporary.path().join("baseline.rs");
    let right = temporary.path().join("local.rs");
    std::fs::write(&left, "baseline\n").unwrap();
    std::fs::write(&right, "local\n").unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");

    cx.update(|window, cx| {
        workspace.update(cx, |view, cx| {
            view.open_comparisons(&[(left.clone(), right.clone())], window, cx)
                .unwrap();
        });
        assert_eq!(workspace.read(cx).tabs.entries.len(), 2);
        assert_eq!(workspace.read(cx).tabs.active, Some(1));
    });
    // Match Perforce removing temporary inputs immediately after acknowledgment.
    std::fs::remove_file(left).unwrap();
    std::fs::remove_file(right).unwrap();

    cx.update(|window, cx| {
        let width = window.find("rows-viewport").bounds().size.width;
        window.click_at("rows-viewport", point(width * 0.75, px(11.0)), cx);
        assert_eq!(copy_active_text(window, cx), "local\n");

        workspace.update(cx, |view, cx| {
            view.open_comparisons(
                &[(fixtures.join("before.rs"), fixtures.join("after.rs"))],
                window,
                cx,
            )
            .unwrap();
        });
        assert_eq!(workspace.read(cx).tabs.entries.len(), 2);
        assert_eq!(workspace.read(cx).tabs.active, Some(0));
        assert_eq!(copy_active_text(window, cx), edited_text);
        assert!(workspace.read(cx).has_modified_tabs(cx));

        let undo = if cfg!(target_os = "macos") {
            "cmd-z"
        } else {
            "ctrl-z"
        };
        // Input simulation types one character per history entry.
        for _ in "changed".chars() {
            window.press(undo, cx);
        }
        assert!(
            !workspace.read(cx).has_modified_tabs(cx),
            "reopening an existing pair must preserve its undo history"
        );
    });
}

#[gpui_kit::test]
fn forwarded_requests_report_errors_and_do_not_interrupt_a_discard_dialog(cx: &mut TestAppContext) {
    let (workspace, cx) = harness(cx);
    let missing = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/does-not-exist.rs");
    cx.update(|window, cx| {
        let error = workspace
            .update(cx, |view, cx| {
                view.open_comparisons(&[(missing.clone(), missing.clone())], window, cx)
            })
            .unwrap_err();
        assert!(error.contains("does-not-exist.rs"));
        assert_eq!(workspace.read(cx).tabs.entries.len(), 1);

        let width = window.find("rows-viewport").bounds().size.width;
        window.click_at("rows-viewport", point(width * 0.75, px(11.0)), cx);
        window.input("changed", cx);
        window.click(("tab-close-target", 0usize), cx);
        assert!(window.has_active_dialog(cx));

        let error = workspace
            .update(cx, |view, cx| {
                view.open_comparisons(&[(missing.clone(), missing.clone())], window, cx)
            })
            .unwrap_err();
        assert!(error.contains("dialog open"));
        workspace
            .update(cx, |view, cx| view.open_comparisons(&[], window, cx))
            .unwrap();
        assert!(window.has_active_dialog(cx));

        // Focusing the existing window must not divert Enter/Escape from its modal.
        window.press("escape", cx);
        assert!(!window.has_active_dialog(cx));
        assert_eq!(workspace.read(cx).tabs.entries.len(), 1);
        assert!(workspace.read(cx).has_modified_tabs(cx));
    });
}

#[gpui_kit::test]
fn native_close_can_discard_edits_and_close_the_window(cx: &mut TestAppContext) {
    let (workspace, cx) = harness(cx);
    cx.update(|window, cx| {
        let width = window.find("rows-viewport").bounds().size.width;
        window.click_at("rows-viewport", point(width * 0.75, px(11.0)), cx);
        window.input("changed", cx);
    });
    cx.run_until_parked();
    cx.update(|_, cx| assert!(workspace.read(cx).has_modified_tabs(cx)));

    assert!(!cx.simulate_close());
    cx.update(|window, cx| {
        window.render_frame(cx);
        assert!(window.has_active_dialog(cx));
        window.click("ok", cx);
    });
    cx.run_until_parked();

    cx.cx.update(|cx| {
        assert!(
            cx.windows().is_empty(),
            "confirming discard must close the window"
        );
    });
}
