use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::table::{
    ColumnSort, DataTable, Table, TableBody, TableCell, TableDelegate, TableEvent, TableHead,
    TableHeader, TableRow, TableState,
};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::path::PathBuf;

use crate::tablelayer::{PAGE_SIZE, TableLayer};
use crate::tabulite::{ClearFilter, DismissFilters, ToggleFilter};
use crate::{
    tableio::{self, LoadingMode},
    utils,
};

pub struct TableView {
    active_tab: usize,
    data_path: Option<PathBuf>,
    layer_names: Vec<SharedString>,
    file_generation: u64,
    layer_generation: u64,
    table: Entity<TableState<TableLayer>>,
    loading_mode: LoadingMode,
    table_bounds: Bounds<Pixels>,
    _table_subscription: Subscription,
    _table_observation: Subscription,
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
        let table_subscription = cx.subscribe(&table, |_, table, event, cx| {
            if let TableEvent::ColumnWidthsChanged(widths) = event {
                table.update(cx, |table, _| {
                    table.delegate_mut().set_column_widths(widths);
                });
            } else if let TableEvent::SelectColumn(col_ix) = event {
                table.update(cx, |table, _| {
                    table.delegate_mut().set_selected_col(*col_ix);
                });
            }
        });

        let table_observation = cx.observe(&table, |_, _, cx| cx.notify());
        let mut view = Self {
            active_tab: 0,
            data_path: None,
            file_generation: 0,
            layer_generation: 0,
            table,
            loading_mode: LoadingMode::Paged,
            table_bounds: Bounds::default(),
            _table_subscription: table_subscription,
            _table_observation: table_observation,
            layer_names: Vec::default(),
        };
        if let Some(path) = path {
            view.load_table(path, cx).detach();
        }
        view
    }

    pub(crate) fn select_previous_column(&mut self, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            let columns_count = table.delegate().columns_count();
            if columns_count == 0 {
                return;
            }

            let selected = table.selected_col().unwrap_or(0);
            let selected = selected.checked_sub(1).unwrap_or(columns_count - 1);
            table.delegate_mut().set_selected_col(selected);
            table.set_selected_col(selected, cx);
        });
    }

    pub(crate) fn select_next_column(&mut self, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            let columns_count = table.delegate().columns_count();
            if columns_count == 0 {
                return;
            }

            let selected = table.selected_col().unwrap_or(0);
            let selected = (selected + 1) % columns_count;
            table.delegate_mut().set_selected_col(selected);
            table.set_selected_col(selected, cx);
        });
    }

    pub(crate) fn select_previous_row(&mut self, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            let rows_count = table.delegate().rows_count();
            if rows_count == 0 {
                return;
            }

            let selected = table.selected_row().unwrap_or(0);
            table.set_selected_row(selected.checked_sub(1).unwrap_or(rows_count - 1), cx);
        });
    }

    pub(crate) fn select_next_row(&mut self, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            let rows_count = table.delegate().rows_count();
            if rows_count == 0 {
                return;
            }

            let selected = table.selected_row().map_or(0, |row| (row + 1) % rows_count);
            table.set_selected_row(selected, cx);
        });
    }

    pub(crate) fn select_first_row(&mut self, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            if table.delegate().rows_count() > 0 {
                table.set_selected_row(0, cx);
            }
        });
    }

    pub(crate) fn select_last_row(&mut self, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            let rows_count = table.delegate().rows_count();
            if rows_count > 0 {
                table.set_selected_row(rows_count - 1, cx);
            }
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

    fn on_action_clear_filter(
        &mut self,
        _: &ClearFilter,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.table.update(cx, |table, cx| {
            let Some(col_ix) = table.selected_col() else {
                return;
            };

            table.delegate_mut().clear_filter(col_ix, window, cx);
        });
    }

    fn on_action_dismiss_filters(
        &mut self,
        _: &DismissFilters,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.table.update(cx, |table, cx| {
            if table.delegate_mut().hide_filters_if_empty(cx) {
                table.refresh_header_layout(cx);
            }
        });
    }

    pub(crate) fn load_table(&mut self, path: PathBuf, cx: &mut Context<Self>) -> Task<()> {
        self.file_generation += 1;
        self.layer_generation += 1;
        let generation = self.file_generation;
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
                        if this.file_generation != generation {
                            return;
                        }
                        this.data_path = Some(path.clone());
                        this.active_tab = 0;
                        this.layer_names = layers;
                        if let Some(layer) = first_layer {
                            this.load_table_layer(path, layer.to_string(), cx).detach();
                        }
                        cx.notify();
                    });
                }
                Err(err) => {
                    if this
                        .read_with(cx, |this, _| this.file_generation == generation)
                        .unwrap_or(false)
                    {
                        utils::error_notification("Failed to load data", err, cx);
                    }
                }
            }
        })
    }

    fn load_table_layer(
        &mut self,
        path: PathBuf,
        layer: String,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        self.layer_generation += 1;
        let generation = self.layer_generation;
        cx.spawn(async move |this, cx| {
            let requested_path = path.clone();
            let requested_layer = layer.clone();
            // Move blocking I/O to a thread pool
            let layer_data = cx
                .background_executor()
                .spawn(async move {
                    let data = tableio::layer_data(&path, &layer)?;
                    let (rows, has_more) = match data.loading_mode {
                        LoadingMode::Eager => (data.query_all(&[], None)?, false),
                        LoadingMode::Paged => {
                            let mut rows = data.query_page(&[], None, 0, PAGE_SIZE + 1)?;
                            let has_more = rows.rows.len() > PAGE_SIZE;
                            rows.rows.truncate(PAGE_SIZE);
                            (rows, has_more)
                        }
                    };
                    anyhow::Ok((data, rows, has_more))
                })
                .await;
            match layer_data {
                Ok((data, rows, has_more)) => {
                    let _ = this.update_in(cx, |this, window, cx| {
                        if this.layer_generation != generation
                            || this.data_path.as_ref() != Some(&requested_path)
                            || this
                                .layer_names
                                .get(this.active_tab)
                                .map(|name| name.as_ref())
                                != Some(requested_layer.as_str())
                        {
                            return;
                        }
                        let initial_count = rows.rows.len() + usize::from(has_more);
                        this.loading_mode = data.loading_mode;
                        let count_source = data.clone();
                        let query_generation = this.table.update(cx, |table, cx| {
                            table.sortable = true;
                            table.delegate_mut().update_data(data, initial_count, rows);
                            table.scroll_to_row(0, cx);
                            let visible = table.visible_range().rows().clone();
                            table.delegate_mut().request_range(visible, cx);
                            table.refresh(cx);
                            cx.notify();
                            table.delegate().generation()
                        });
                        this.table.focus_handle(cx).focus(window, cx);
                        if has_more {
                            cx.spawn(async move |this, cx| {
                                let count = cx
                                    .background_executor()
                                    .spawn(async move { count_source.count(&[]) })
                                    .await;
                                match count {
                                    Ok(count) => {
                                        let _ = this.update(cx, |this, cx| {
                                            if this.layer_generation != generation {
                                                return;
                                            }
                                            this.table.update(cx, |table, cx| {
                                                if table.delegate().generation() != query_generation
                                                {
                                                    return;
                                                }
                                                table.delegate_mut().set_row_count(count);
                                                let visible = table.visible_range().rows().clone();
                                                table.delegate_mut().request_range(visible, cx);
                                                table.refresh(cx);
                                                cx.notify();
                                            });
                                        });
                                    }
                                    Err(err) => log::error!("Failed to count table rows: {err}"),
                                }
                            })
                            .detach();
                        }
                    });
                }
                Err(err) => {
                    if !this
                        .read_with(cx, |this, _| this.layer_generation == generation)
                        .unwrap_or(false)
                    {
                        return;
                    }
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

    fn render_eager_content(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let column_count = self.table.read(cx).delegate().columns_count();
        let filter_inputs = self.table.update(cx, |table, cx| {
            (0..column_count)
                .map(|ix| table.delegate_mut().filter_input(ix, window, cx))
                .collect::<Vec<_>>()
        });
        let state = self.table.read(cx);
        let delegate = state.delegate();
        let mut header = TableRow::new();
        for (ix, input) in filter_inputs.into_iter().enumerate() {
            let column = delegate.column(ix, cx);
            let table = self.table.clone();
            let sort_label = match column.sort {
                Some(ColumnSort::Ascending) => format!("{} ↑", column.name),
                Some(ColumnSort::Descending) => format!("{} ↓", column.name),
                _ => column.name.to_string(),
            };
            let title = div()
                .id(("eager-sort", ix))
                .cursor_pointer()
                .child(SharedString::new(sort_label))
                .on_click(move |_, window, cx| {
                    table.update(cx, |state, cx| {
                        state.set_selected_col(ix, cx);
                        let next = match state.delegate().column(ix, cx).sort {
                            Some(ColumnSort::Ascending) => ColumnSort::Descending,
                            Some(ColumnSort::Descending) => ColumnSort::Default,
                            _ => ColumnSort::Ascending,
                        };
                        state.delegate_mut().perform_sort(ix, next, window, cx);
                    });
                });
            header = header.child(
                TableHead::new().w(column.width).child(
                    div()
                        .flex()
                        .flex_col()
                        .child(title)
                        .child(gpui_kit::component::input::Input::new(&input).xsmall()),
                ),
            );
        }
        let mut body = TableBody::new();
        for row_ix in 0..delegate.rows_count() {
            let mut row = TableRow::new().when(state.selected_row() == Some(row_ix), |row| {
                row.bg(cx.theme().tokens.table_active)
            });
            for col_ix in 0..column_count {
                let column = delegate.column(col_ix, cx);
                let text = delegate.cell(row_ix, col_ix).flatten().unwrap_or("null");
                let table = self.table.clone();
                let mut cell = TableCell::new().w(column.width).child(
                    div()
                        .id(("eager-cell", row_ix * column_count + col_ix))
                        .size_full()
                        .child(SharedString::new(text))
                        .on_click(move |_, _, cx| {
                            table.update(cx, |state, cx| state.set_selected_row(row_ix, cx));
                        }),
                );
                if column.align == TextAlign::Right {
                    cell = cell.text_right();
                } else if column.align == TextAlign::Center {
                    cell = cell.text_center();
                }
                row = row.child(cell);
            }
            body = body.child(row);
        }
        div()
            .size_full()
            .track_focus(&self.table.focus_handle(cx))
            .on_action(cx.listener(Self::on_action_filter))
            .on_action(cx.listener(Self::on_action_clear_filter))
            .on_action(cx.listener(Self::on_action_dismiss_filters))
            .overflow_scrollbar()
            .child(
                Table::new()
                    .xsmall()
                    .child(TableHeader::new().child(header))
                    .child(body),
            )
    }

    fn render_tab_content(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        div()
            .flex()
            .flex_1()
            .size_full()
            .child(DataTable::new(&self.table).xsmall())
            .on_action(cx.listener(Self::on_action_filter))
            .on_action(cx.listener(Self::on_action_clear_filter))
            .on_action(cx.listener(Self::on_action_dismiss_filters))
            .on_prepaint(move |bounds, _, cx| {
                view.update(cx, |view, cx| {
                    if view.table_bounds != bounds {
                        view.table_bounds = bounds;
                        cx.notify();
                    }
                });
            })
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
                view.load_table_layer(path, layer_name, cx).detach();
                cx.notify();
            }));

        for layer in &self.layer_names {
            tab_bar = tab_bar.child(Tab::new().label(layer.clone()));
        }

        v_flex()
            .size_full()
            .child(div().flex_1().size_full().child(match self.loading_mode {
                LoadingMode::Eager => self.render_eager_content(window, cx).into_any_element(),
                LoadingMode::Paged => self.render_tab_content(window, cx).into_any_element(),
            }))
            .child(tab_bar)
    }
}
