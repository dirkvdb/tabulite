use gpui::*;
use gpui::{App, IntoElement, Window};
use gpui_component::tab::{Tab, TabBar};
use gpui_component::table::{Table, TableState};
use gpui_component::*;
use std::path::PathBuf;

use crate::tableio;
use crate::tablelayer::TableLayer;

pub struct TableView {
    active_tab: usize,
    data_path: Option<PathBuf>,
    layer_names: Vec<SharedString>,
    table: Entity<TableState<TableLayer>>,
    _load_task: Task<()>,
}

impl TableView {
    pub fn view(path: Option<PathBuf>, window: &mut Window, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self::new(path, window, cx))
    }

    fn new(path: Option<PathBuf>, window: &mut Window, cx: &mut gpui::Context<Self>) -> Self {
        let table = cx.new(|cx| TableState::new(TableLayer::default(), window, cx));

        if let Some(path) = path {
            if let Ok(layer_names) = tableio::layers_for_path(&path) {
                let task = if let Some(current_layer) = layer_names.first() {
                    Self::load_table_layer_task(path.clone(), current_layer.to_string(), cx)
                } else {
                    Task::ready(())
                };

                return Self {
                    active_tab: 0,
                    data_path: Some(path),
                    table,
                    layer_names,
                    _load_task: task,
                };
            }
        }

        Self {
            active_tab: 0,
            data_path: None,
            table,
            layer_names: Vec::default(),
            _load_task: Task::ready(()),
        }
    }

    fn load_table_layer_task(
        path: PathBuf,
        layer: String,
        cx: &mut gpui::Context<Self>,
    ) -> Task<()> {
        cx.spawn(async move |this, cx| {
            let layer_data = tableio::layer_data(&path, &layer).unwrap_or_default();

            let _ = this.update(cx, |this, cx| {
                this.table.update(cx, |table, cx| {
                    table.delegate_mut().update_data(layer_data);
                    table.refresh(cx);
                });
                cx.notify();
            });
        })
    }

    fn render_tab_content(
        &self,
        _window: &mut Window,
        _cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        div()
            .flex()
            .flex_1()
            .size_full()
            .child(Table::new(&self.table).stripe(true).xsmall())
    }
}

impl Render for TableView {
    fn render(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        if self.layer_names.is_empty() {
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
                let path = view.data_path.clone().unwrap();
                let layer_name = view.layer_names[*index].to_string();
                view._load_task = Self::load_table_layer_task(path, layer_name, cx);
                cx.notify();
            }));

        for layer in &self.layer_names {
            tab_bar = tab_bar.child(Tab::new().label(layer.clone()));
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
