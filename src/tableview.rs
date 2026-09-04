use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::table::{DataTable, TableState};
use gpui_kit::component::*;
use gpui_kit::*;
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

    fn new(path: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let table = cx.new(|cx| TableState::new(TableLayer::default(), window, cx));
        table.update(cx, |table, cx| {
            let focus_handle = table.focus_handle(cx);
            table.delegate_mut().set_table_focus_handle(focus_handle);
        });

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

    pub(crate) fn select_previous_column(&mut self, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            let columns_count = table.delegate().columns_count();
            if columns_count == 0 {
                return;
            }

            let selected = table.selected_col().unwrap_or(0);
            table.set_selected_col(selected.checked_sub(1).unwrap_or(columns_count - 1), cx);
        });
    }

    pub(crate) fn select_next_column(&mut self, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            let columns_count = table.delegate().columns_count();
            if columns_count == 0 {
                return;
            }

            let selected = table.selected_col().unwrap_or(0);
            table.set_selected_col((selected + 1) % columns_count, cx);
        });
    }

    pub(crate) fn focus_selected_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            let Some(col_ix) = table.selected_col() else {
                return;
            };

            let input = table.delegate_mut().filter_input(col_ix, window, cx);
            table.refresh(cx);
            cx.notify();
            input.update(cx, |input, cx| input.focus(window, cx));
        });
    }

    fn on_action_filter(&mut self, _: &ToggleFilter, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_selected_filter(window, cx);
    }

    pub(crate) fn load_table(path: PathBuf, cx: &mut Context<Self>) -> Task<()> {
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

    fn load_table_layer(path: PathBuf, layer: String, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            // Move blocking I/O to a thread pool
            let layer_data = cx
                .background_executor()
                .spawn(async move { tableio::layer_data(&path, &layer) })
                .await;
            match layer_data {
                Ok(data) => {
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.table.update(cx, |table, cx| {
                            table.sortable = true;
                            table.delegate_mut().update_data(data);
                            table.refresh(cx);
                            if table.delegate().columns_count() > 0 {
                                table.set_selected_col(0, cx);
                            }
                            cx.notify();
                        });
                        this.table.focus_handle(cx).focus(window, cx);
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

    fn render_tab_content(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_1()
            .size_full()
            .child(DataTable::new(&self.table).stripe(true).xsmall())
            .on_action(cx.listener(Self::on_action_filter))
    }
}

impl Render for TableView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
    }
}
