use gpui::*;
use gpui_component::*;
use gpui_component_assets::Assets;
use std::path::PathBuf;
use tabulite::{appconfig, tabulite::Tabulite};

fn main() {
    let app = Application::new().with_assets(Assets);

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
        tabulite::init(cx);

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
                let app = Tabulite::view(args.input_file, window, cx);
                cx.new(|cx| Root::new(app, window, cx))
            })?;

            Ok::<_, anyhow::Error>(())
        })
        .detach();
    });
}
