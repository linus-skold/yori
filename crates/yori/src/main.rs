//! yori process startup and native application composition.

mod editor;

use editor::{AlignedEditor, PaneDocument};
use gpui_kit::component::{
    Root,
    theme::{Theme, ThemeMode},
};
use gpui_kit::{AppContext, WindowOptions};
use std::{env, path::PathBuf, process};
use yori_document::Document;

fn usage(program: &str) -> String {
    format!("usage: {program} <left-file> <right-file>")
}

fn load_arguments() -> Result<(PaneDocument, PaneDocument), String> {
    let mut args = env::args_os();
    let program = args
        .next()
        .and_then(|value| value.into_string().ok())
        .unwrap_or_else(|| "yori".to_owned());
    let left = args.next().map(PathBuf::from);
    let right = args.next().map(PathBuf::from);
    if left.is_none() || right.is_none() || args.next().is_some() {
        return Err(usage(&program));
    }
    let left_path = left.unwrap();
    let right_path = right.unwrap();
    let left_document = Document::read(&left_path)?;
    let right_document = Document::read(&right_path)?;
    Ok((
        PaneDocument::new(left_path, left_document),
        PaneDocument::new(right_path, right_document),
    ))
}

fn main() {
    let (left, right) = load_arguments().unwrap_or_else(|error| {
        eprintln!("yori: {error}");
        process::exit(2);
    });

    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        Theme::change(ThemeMode::Dark, None, cx);
        editor::init(cx);
        cx.spawn(async move |cx| {
            cx.open_window(WindowOptions::default(), |window, cx| {
                let editor = cx.new(|cx| AlignedEditor::new(left, right, window, cx));
                cx.new(|cx| Root::new(editor, window, cx))
            })
            .expect("failed to open yori window");
        })
        .detach();
    });
}
