use gpui_component::*;
use std::path::PathBuf;
use crate::tableview::TableView;
use gpui::*;

mod appconfig;
mod tableio;
mod tablelayer;
mod tableview;

fn main() {
    let app = Application::new();

    use clap::Parser;

    #[derive(Parser, Debug)]
    #[command(author, version, about, long_about = None)]
    struct Args {
        input_file: Option<std::path::PathBuf>,
        #[arg(short = 'c', long = "config")]
        config_file: Option<std::path::PathBuf>,
    }

    let args = Args::parse();
    let config = appconfig::load_config(args.config_file.as_deref());

    app.run(move |cx| {
        gpui_component::init(cx);

        let theme_name = SharedString::from(config.theme);
        let themes_dir = std::env::var("CARGO_MANIFEST_DIR")
            .map(|dir| PathBuf::from(dir).join("themes"))
            .unwrap_or_else(|_| PathBuf::from("./themes"));
        if let Err(err) = ThemeRegistry::watch_dir(themes_dir, cx, move |cx| {
            if let Some(theme) = ThemeRegistry::global(cx).themes().get(&theme_name).cloned() {
                Theme::global_mut(cx).apply_config(&theme);
            }
        }) {
            log::error!("Failed to watch themes directory: {}", err);
        }

        cx.spawn(async move |cx| {
            cx.open_window(WindowOptions::default(), |window, cx| {
                let table = TableView::view(args.input_file, window, cx);

                // This first level on the window, should be a Root.
                cx.new(|cx| Root::new(table, window, cx))
            })?;

            Ok::<_, anyhow::Error>(())
        })
        .detach();
    });
}
