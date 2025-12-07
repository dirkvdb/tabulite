use geo::vector::dataframe::Field;
use gpui::{App, IntoElement, Window};
use gpui_component::tab::{Tab, TabBar};
use gpui_component::table::Table;
use gpui_component::*;
use std::path::PathBuf;

use gpui::*;
use gpui_component::table::{Column, TableDelegate, TableState};

mod appconfig;
mod tableio;

#[derive(Clone)]
pub struct TableLayer {
    name: SharedString,
    data: Vec<Vec<Option<Field>>>,
    columns: Vec<Column>,
}

impl TableDelegate for TableLayer {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.data.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> &Column {
        &self.columns[col_ix]
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        _: &mut gpui::Context<'_, TableState<Self>>,
    ) -> impl IntoElement {
        let row = &self.data[row_ix];

        match &row[col_ix] {
            Some(field) => match field {
                Field::String(s) => s.clone(),
                Field::Integer(i) => i.to_string(),
                Field::Float(f) => f.to_string(),
                Field::Boolean(b) => b.to_string(),
                Field::DateTime(naive_date_time) => naive_date_time.to_string(),
            },
            None => String::default(),
        }
    }
}

pub struct TableView {
    active_tab: usize,
    layers: Vec<Entity<TableState<TableLayer>>>,
}

impl TableView {
    pub fn view(path: Option<PathBuf>, window: &mut Window, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self::new(path, window, cx))
    }

    fn new(path: Option<PathBuf>, window: &mut Window, cx: &mut gpui::Context<Self>) -> Self {
        let layers = match path {
            Some(path) => tableio::load_file(path)
                .unwrap_or_default()
                .into_iter()
                .map(|layer| cx.new(|cx| TableState::new(layer, window, cx)))
                .collect(),
            None => Vec::new(),
        };

        Self {
            active_tab: 0,
            layers,
        }
    }

    fn render_tab_content(
        &self,
        _window: &mut Window,
        _cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        self.layers.get(self.active_tab).map_or_else(
            || div().child("No layer selected"),
            |layer| {
                div()
                    .flex()
                    .flex_1()
                    .size_full()
                    .child(Table::new(layer).stripe(true).xsmall())
            },
        )
    }
}

impl Render for TableView {
    fn render(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        if self.layers.is_empty() {
            return div().grid().size_full().content_center().child(
                div()
                    .font_bold()
                    .text_center()
                    .child("No data loaded")
                    .child("Press Ctrl+o to open a file"),
            );
        }

        let mut tab_bar = TabBar::new("layers")
            .selected_index(self.active_tab)
            .on_click(cx.listener(|view, index, _, cx| {
                view.active_tab = *index;
                cx.notify();
            }));

        for layer in &self.layers {
            let x = layer.read(cx);
            tab_bar = tab_bar.child(Tab::new().label(x.delegate().name.clone()));
        }

        v_flex()
            .size_full()
            .child(
                div()
                    .flex_1()
                    .size_full()
                    .child(self.render_tab_content(window, cx)),
            )
            .child(tab_bar)
    }
}

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
