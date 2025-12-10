use gpui::*;
use gpui::{App, IntoElement, Window};
use gpui_component::kbd::Kbd;
use gpui_component::notification::Notification;
use gpui_component::tab::{Tab, TabBar};
use gpui_component::table::{Table, TableState};
use gpui_component::*;
use std::path::PathBuf;

use crate::tablelayer::TableLayer;
use crate::tabulite::ToggleFilter;
use crate::{tableio, utils};

pub struct TableView {
    active_tab: usize,
    data_path: Option<PathBuf>,
    layer_names: Vec<SharedString>,
    table: Entity<TableState<TableLayer>>,
}

impl TableView {
    pub fn view(path: Option<PathBuf>, window: &mut Window, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self::new(path, window, cx))
    }

    fn new(path: Option<PathBuf>, window: &mut Window, cx: &mut gpui::Context<Self>) -> Self {
        let table = cx.new(|cx| TableState::new(TableLayer::default(), window, cx));

        if let Some(path) = path {
            Self::load_table(path, cx).detach();
        }

        Self {
            active_tab: 0,
            data_path: None,
            table,
            layer_names: Vec::default(),
        }
    }

    fn on_action_toggle_search(
        &mut self,
        _: &ToggleFilter,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.table.update(cx, |table, cx| {
            table.sortable = true;
            table.delegate_mut().toggle_filter();
            table.refresh(cx);
        });

        cx.propagate();
    }

    fn load_table(path: PathBuf, cx: &mut gpui::Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            let path_clone = path.clone();
            let layers: Result<_> = cx
                .background_executor()
                .spawn(async move { Ok(tableio::layers_for_path(&path_clone)?) })
                .await;

            match layers {
                Ok(layers) => {
                    let first_layer = layers.first().cloned();
                    let _ = this.update(cx, |this, cx| {
                        this.data_path = Some(path.clone());
                        this.layer_names = layers;
                        cx.notify();
                    });

                    if let Some(layer) = first_layer {
                        let _ = this.update(cx, |_this, cx| {
                            Self::load_table_layer(path, layer.to_string(), cx).detach();
                        });
                    }
                }
                Err(err) => {
                    utils::error_notification("Failed to load data", err, cx);
                }
            }
        })
    }

    fn load_table_layer(path: PathBuf, layer: String, cx: &mut gpui::Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            // Move blocking I/O to a thread pool
            let layer_data = cx
                .background_executor()
                .spawn(async move { tableio::layer_data(&path, &layer) })
                .await;
            match layer_data {
                Ok(data) => {
                    let _ = this.update(cx, |this, cx| {
                        this.table.update(cx, |table, cx| {
                            table.sortable = true;
                            table.delegate_mut().update_data(data);
                            table.refresh(cx);
                            cx.notify();
                        });
                    });
                }
                Err(err) => {
                    let _ = cx.update(|app| {
                        if let Some(window_handle) = app.active_window() {
                            let _ = app.update_window(window_handle, |_, window, app| {
                                let message =
                                    SharedString::new(format!("Failed to load data': {err}"));

                                window.push_notification(Notification::error(message), app);
                            });
                        }
                    });
                    return;
                }
            };
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
            #[cfg(target_os = "macos")]
            let shortcut_hint = "cmd+o";
            #[cfg(not(target_os = "macos"))]
            let shortcut_hint = "ctrl-o";
            return div().grid().size_full().content_center().child(
                v_flex()
                    .font_bold()
                    .text_center()
                    .child("No data loaded")
                    .child(
                        h_flex()
                            .justify_center()
                            .debug_pink()
                            .gap_2()
                            .child("Press")
                            .child(Kbd::new(Keystroke::parse(shortcut_hint).unwrap()))
                            .child("to open a file"),
                    ),
            );
        }

        let mut tab_bar = TabBar::new("layers")
            .selected_index(self.active_tab)
            .on_click(cx.listener(|view, index, _, cx| {
                view.active_tab = *index;
                let path = view.data_path.clone().unwrap();
                let layer_name = view.layer_names[*index].to_string();
                Self::load_table_layer(path, layer_name, cx).detach();
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
            .on_action(cx.listener(Self::on_action_toggle_search))
    }
}
