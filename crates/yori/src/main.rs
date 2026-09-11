//! yori process startup and native application composition.

mod appearance;
mod editor;
mod instance;
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
    let paths = args
        .map(|path| {
            std::path::absolute(&path).map_err(|error| {
                format!("cannot resolve {}: {error}", PathBuf::from(path).display())
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
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

fn dispatch_open<C: AppContext>(
    window: gpui_kit::WindowHandle<Root>,
    workspace: &gpui_kit::Entity<Workspace>,
    pairs: &[(PathBuf, PathBuf)],
    cx: &mut C,
) -> Result<(), String> {
    // The typed handle also mutably borrows Root. Workspace opening must be
    // able to read/update Root itself for dialog checks and error notifications.
    let window: gpui_kit::AnyWindowHandle = window.into();
    window
        .update(cx, |_, window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.open_comparisons(pairs, window, cx)
            })
        })
        .unwrap_or_else(|error| Err(format!("yori's window closed: {error}")))
}

fn main() {
    let pairs = load_arguments().unwrap_or_else(|error| {
        eprintln!("yori: {error}");
        process::exit(2);
    });

    let Some(instance) = instance::Instance::start(&pairs).unwrap_or_else(|error| {
        eprintln!("yori: {error}");
        process::exit(1);
    }) else {
        return;
    };

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
                let mut workspace = None;
                let window = cx
                    .open_window(WindowOptions::default(), |window, cx| {
                        let view = cx.new(|cx| Workspace::new(window, cx));
                        workspace = Some(view.clone());
                        cx.new(|cx| Root::new(view, window, cx))
                    })
                    .expect("failed to open yori window");
                let workspace = workspace.expect("workspace initialized with its window");

                // Root is installed now, so error notifications and editor focus
                // are available before handling either initial or forwarded files.
                let initial = dispatch_open(window, &workspace, &pairs, cx);
                if let Err(error) = initial {
                    eprintln!("yori: {error}");
                }

                while let Ok(request) = instance.next().await {
                    let result = if request.expired() {
                        Err("request expired before yori could open it; retry".into())
                    } else {
                        dispatch_open(window, &workspace, &request.pairs, cx)
                    };

                    request.complete(result);
                }
            })
            .detach();
        });
}
