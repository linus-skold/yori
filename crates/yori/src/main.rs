//! yori process startup and native application composition.

mod appearance;
mod editor;
mod workspace;
use gpui_kit::component::Root;
use gpui_kit::{AppContext, WindowOptions};
use std::{env, path::PathBuf, process};
use workspace::Workspace;

fn usage(program: &str) -> String {
    format!("usage: {program} [<left-file> <right-file>]...")
}

fn load_arguments() -> Result<Vec<(PathBuf, PathBuf)>, String> {
    let mut args = env::args_os();
    let program = args
        .next()
        .and_then(|value| value.into_string().ok())
        .unwrap_or_else(|| "yori".to_owned());
    let paths = args.map(PathBuf::from).collect::<Vec<_>>();
    if !paths.len().is_multiple_of(2) {
        return Err(usage(&program));
    }

    Ok(paths
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| (pair[0].clone(), pair[1].clone()))
        .collect())
}

fn main() {
    let pairs = load_arguments().unwrap_or_else(|error| {
        eprintln!("yori: {error}");
        process::exit(2);
    });

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            appearance::init(cx);
            editor::init(cx);
            workspace::init(cx);
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();

            cx.spawn(async move |cx| {
                cx.open_window(WindowOptions::default(), |window, cx| {
                    let workspace = cx.new(|cx| Workspace::new(window, cx));
                    let root = cx.new(|cx| Root::new(workspace.clone(), window, cx));
                    window.defer(cx, move |window, cx| {
                        workspace.update(cx, |workspace, cx| {
                            for (left, right) in pairs {
                                workspace.open_paths(&left, &right, window, cx);
                            }
                        });
                    });

                    root
                })
                .expect("failed to open yori window");
            })
            .detach();
        });
}
