use crate::tableio::{ANY_COLUMN, ColumnKind, LoadingMode, TableData, TableRows};
use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    time::Duration,
};

use gpui_kit::component::{
    ActiveTheme, Icon, IconName, Sizable, Size, StyleSized,
    input::{Enter, Escape, Input, InputEvent, InputState},
    menu::{ContextMenuExt, PopupMenu, PopupMenuItem},
    table::{Column, ColumnGroup, ColumnSort, TableDelegate, TableState},
    tag::Tag,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

#[derive(Default)]
pub struct TableLayer {
    data: HashMap<usize, Vec<Vec<Option<String>>>>,
    pending_pages: HashSet<usize>,
    first_page_pending: bool,
    row_count: usize,
    visible_start: usize,
    visible_end: usize,
    filters: Vec<(usize, String)>,
    source: Option<TableData>,
    sort: Option<(usize, bool)>,
    query_generation: u64,
    filter_enabled: bool,
    filter_inputs: Vec<Entity<InputState>>,
    any_filter_visible: bool,
    any_filter_input: Option<Entity<InputState>>,
    input_subscriptions: Vec<Subscription>,
    columns: Vec<Column>,
    table_focus_handle: Option<FocusHandle>,
    selected_col: Option<usize>,
}

const NULL: &str = "null";
pub(crate) const PAGE_SIZE: usize = 128;

pub(crate) fn row_number(row_ix: usize) -> String {
    (row_ix + 1).to_string()
}

pub(crate) fn row_number_width(row_count: usize) -> Pixels {
    let digits = row_count.max(1).to_string().len() as f32;
    px((digits * 8.0 + 24.0).max(40.0))
}
const MAX_CACHED_PAGES: usize = 8;
const MAX_PENDING_PAGES: usize = 2;

pub(crate) fn cell_filter_menu(
    mut menu: PopupMenu,
    table: Entity<TableState<TableLayer>>,
    col_ix: usize,
    value: String,
    generation: u64,
) -> PopupMenu {
    for (label, expression) in [
        ("Use as Exact Filter", format!("={value}")),
        ("Use in Filter Expression: not equal", format!("<>{value}")),
        (
            "Use in Filter Expression: greater than",
            format!(">{value}"),
        ),
        (
            "Use in Filter Expression: greater or equal",
            format!(">={value}"),
        ),
        ("Use in Filter Expression: less than", format!("<{value}")),
        (
            "Use in Filter Expression: less or equal",
            format!("<={value}"),
        ),
    ] {
        let table = table.clone();
        menu = menu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
            table.update(cx, |state, cx| {
                if state.delegate().generation() != generation {
                    return;
                }
                let input = state.delegate_mut().filter_input(col_ix, window, cx);
                input.update(cx, |input, cx| {
                    input.set_value(expression.clone(), window, cx)
                });
                state.delegate_mut().filter_data(cx);
                state.refresh_header_layout(cx);
            });
        }));
    }
    menu
}

impl TableLayer {
    pub fn update_data(&mut self, data: TableData, row_count: usize, first_page: TableRows) {
        let eager = data.loading_mode == LoadingMode::Eager;
        self.source = Some(data);
        self.data.clear();
        if eager {
            self.store_rows(first_page);
        } else if row_count > 0 {
            self.data.insert(0, first_page.rows);
        }
        self.pending_pages.clear();
        self.first_page_pending = false;
        self.row_count = row_count;
        self.visible_start = 0;
        self.visible_end = 0;
        self.filters.clear();
        self.filter_enabled = false;
        self.any_filter_visible = false;
        self.sort = None;
        self.query_generation += 1;
        self.create_column_info();
    }

    fn store_rows(&mut self, rows: TableRows) {
        self.data.clear();
        for (ix, row) in rows.rows.into_iter().enumerate() {
            self.data
                .entry(ix / PAGE_SIZE * PAGE_SIZE)
                .or_default()
                .push(row);
        }
    }

    pub fn columns_count(&self) -> usize {
        self.columns.len()
    }

    pub fn rows_count(&self) -> usize {
        self.row_count
    }

    pub fn filters_visible(&self) -> bool {
        self.filter_enabled
    }

    pub fn any_filter_visible(&self) -> bool {
        self.any_filter_visible
    }

    pub(crate) fn generation(&self) -> u64 {
        self.query_generation
    }

    pub(crate) fn set_row_count(&mut self, count: usize) {
        self.row_count = count;
        if let Some(column) = self.columns.first_mut() {
            column.width = row_number_width(count);
        }
        self.data.retain(|page, _| *page < count);
        self.pending_pages.retain(|page| *page < count);
    }

    pub fn set_table_focus_handle(&mut self, focus_handle: FocusHandle) {
        self.table_focus_handle = Some(focus_handle);
    }

    pub fn set_selected_col(&mut self, col_ix: usize) {
        self.selected_col = Some(col_ix);
    }

    pub fn set_column_widths(&mut self, widths: &[Pixels]) {
        for (column, width) in self.columns.iter_mut().skip(1).zip(widths.iter().skip(1)) {
            column.width = *width;
        }
    }

    pub fn filter_input(
        &mut self,
        col_ix: usize,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Entity<InputState> {
        self.filter_enabled = true;
        self.ensure_filter_inputs(window, cx);
        self.filter_inputs[col_ix - 1].clone()
    }

    pub fn any_filter_input(
        &mut self,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Entity<InputState> {
        self.filter_enabled = true;
        self.any_filter_visible = true;
        if let Some(input) = &self.any_filter_input {
            return input.clone();
        }
        let input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Filter in any column (words)"));
        self.input_subscriptions
            .push(cx.subscribe(&input, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.delegate_mut().filter_data(cx);
                }
            }));
        self.any_filter_input = Some(input.clone());
        input
    }

    pub fn clear_filter(
        &mut self,
        col_ix: usize,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        let Some(input) = col_ix
            .checked_sub(1)
            .and_then(|ix| self.filter_inputs.get(ix))
            .cloned()
        else {
            return;
        };

        input.update(cx, |input, cx| input.set_value("", window, cx));
        self.filter_data(cx);
    }

    pub fn hide_filters_if_empty(&mut self, cx: &App) -> bool {
        if !self.filter_enabled
            || self
                .filter_inputs
                .iter()
                .any(|input| !input.read(cx).value().is_empty())
            || self
                .any_filter_input
                .as_ref()
                .is_some_and(|input| !input.read(cx).value().is_empty())
        {
            return false;
        }

        self.filter_enabled = false;
        self.any_filter_visible = false;
        true
    }

    fn ensure_filter_inputs(&mut self, window: &mut Window, cx: &mut Context<TableState<Self>>) {
        if !self.filter_inputs.is_empty() {
            return;
        }

        for _ in 1..self.columns.len() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Filter..."));
            self.input_subscriptions.push(cx.subscribe(
                &input,
                |this, entity, event: &InputEvent, cx| {
                    this.delegate_mut()
                        .on_filter_input_event(&entity, event, cx);
                },
            ));
            self.filter_inputs.push(input);
        }
    }

    fn filter_data(&mut self, cx: &mut Context<TableState<Self>>) {
        let mut filters: Vec<_> = self
            .filter_inputs
            .iter()
            .enumerate()
            .filter_map(|(ix, input)| {
                let text = input.read(cx).value().to_string();
                (!text.is_empty()).then_some((ix, text))
            })
            .collect();
        if let Some(input) = &self.any_filter_input {
            filters.extend(
                input
                    .read(cx)
                    .value()
                    .split_whitespace()
                    .map(|word| (ANY_COLUMN, word.to_owned())),
            );
        }
        self.refresh_data(filters, cx);
    }

    fn refresh_data(&mut self, filters: Vec<(usize, String)>, cx: &mut Context<TableState<Self>>) {
        let Some(source) = self.source.clone() else {
            return;
        };
        let filter_changed = filters != self.filters;
        self.filters = filters.clone();
        self.query_generation += 1;
        let generation = self.query_generation;
        let sort = self.sort;
        self.data.clear();
        self.pending_pages.clear();
        self.first_page_pending = true;
        if source.loading_mode == LoadingMode::Eager {
            self.row_count = 0;
        }
        cx.notify();
        cx.spawn(async move |table_state, cx| {
            if filter_changed {
                cx.background_executor()
                    .timer(Duration::from_millis(180))
                    .await;
                if !table_state
                    .read_with(cx, |table, _| {
                        table.delegate().query_generation == generation
                    })
                    .unwrap_or(false)
                {
                    return;
                }
            }
            if source.loading_mode == LoadingMode::Eager {
                let result = cx
                    .background_executor()
                    .spawn(async move { source.query_all(&filters, sort) })
                    .await;
                match result {
                    Ok(rows) => {
                        let _ = table_state.update(cx, |table_state, cx| {
                            let delegate = table_state.delegate_mut();
                            if delegate.query_generation != generation {
                                return;
                            }
                            delegate.set_row_count(rows.rows.len());
                            delegate.store_rows(rows);
                            delegate.first_page_pending = false;
                            table_state.scroll_to_row(0, cx);
                            table_state.refresh(cx);
                            cx.notify();
                        });
                    }
                    Err(err) => log::error!("Failed to query table: {err}"),
                }
                return;
            }
            let page_source = source.clone();
            let page_filters = filters.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let mut rows = page_source.query_page(&page_filters, sort, 0, PAGE_SIZE + 1)?;
                    let has_more = rows.rows.len() > PAGE_SIZE;
                    rows.rows.truncate(PAGE_SIZE);
                    anyhow::Ok((rows, has_more))
                })
                .await;
            match result {
                Ok((rows, has_more)) => {
                    let initial_count = rows.rows.len() + usize::from(has_more);
                    let updated = table_state
                        .update(cx, |table_state, cx| {
                            let delegate = table_state.delegate_mut();
                            if delegate.query_generation != generation {
                                return false;
                            }
                            delegate.set_row_count(initial_count);
                            delegate.first_page_pending = false;
                            if initial_count > 0 {
                                delegate.data.insert(0, rows.rows);
                            }
                            delegate.visible_start = 0;
                            delegate.visible_end = 0;
                            table_state.scroll_to_row(0, cx);
                            let visible = table_state.visible_range().rows().clone();
                            table_state.delegate_mut().request_range(visible, cx);
                            table_state.refresh(cx);
                            cx.notify();
                            true
                        })
                        .unwrap_or(false);
                    if updated && has_more {
                        let count = cx
                            .background_executor()
                            .spawn(async move { source.count(&filters) })
                            .await;
                        match count {
                            Ok(count) => {
                                let _ = table_state.update(cx, |table_state, cx| {
                                    if table_state.delegate().query_generation != generation {
                                        return;
                                    }
                                    table_state.delegate_mut().set_row_count(count);
                                    let visible = table_state.visible_range().rows().clone();
                                    table_state.delegate_mut().request_range(visible, cx);
                                    table_state.refresh(cx);
                                    cx.notify();
                                });
                            }
                            Err(err) => log::error!("Failed to count table rows: {err}"),
                        }
                    }
                }
                Err(err) => log::error!("Failed to query table: {err}"),
            }
        })
        .detach();
    }

    pub(crate) fn cell(&self, row_ix: usize, col_ix: usize) -> Option<Option<&str>> {
        self.data
            .get(&(row_ix / PAGE_SIZE * PAGE_SIZE))?
            .get(row_ix % PAGE_SIZE)
            .map(|row| row[col_ix].as_deref())
    }

    pub(crate) fn cell_badge(&self, row_ix: usize, col_ix: usize) -> Option<String> {
        let value = self.cell(row_ix, col_ix)?;
        let kind = self.source.as_ref()?.columns[col_ix].kind;
        badge_label(kind, value)
    }

    pub(crate) fn request_range(
        &mut self,
        range: Range<usize>,
        cx: &mut Context<TableState<Self>>,
    ) {
        self.visible_start = range.start;
        self.visible_end = range.end;
        if self.first_page_pending
            || self
                .source
                .as_ref()
                .is_some_and(|source| source.loading_mode == LoadingMode::Eager)
        {
            return;
        }
        let Some(source) = self.source.clone() else {
            return;
        };
        let end = range.end.min(self.row_count);
        if end <= range.start {
            return;
        }
        let first = range.start / PAGE_SIZE * PAGE_SIZE;
        let last = (end - 1) / PAGE_SIZE * PAGE_SIZE;
        let before = first.checked_sub(PAGE_SIZE);
        let after = last
            .checked_add(PAGE_SIZE)
            .filter(|page| *page < self.row_count);
        for page in (first..=last).step_by(PAGE_SIZE).chain(before).chain(after) {
            if self.pending_pages.len() >= MAX_PENDING_PAGES {
                break;
            }
            if self.data.contains_key(&page) || !self.pending_pages.insert(page) {
                continue;
            }
            let source = source.clone();
            let filters = self.filters.clone();
            let sort = self.sort;
            let generation = self.query_generation;
            cx.spawn(async move |table_state, cx| {
                let result = cx
                    .background_executor()
                    .spawn(async move { source.query_page(&filters, sort, page, PAGE_SIZE) })
                    .await;
                let _ = table_state.update(cx, |table_state, cx| {
                    let delegate = table_state.delegate_mut();
                    if delegate.query_generation != generation {
                        return;
                    }
                    delegate.pending_pages.remove(&page);
                    match result {
                        Ok(rows) => {
                            delegate.data.insert(page, rows.rows);
                            delegate.evict_distant_pages();
                            let visible = delegate.visible_start..delegate.visible_end;
                            delegate.request_range(visible, cx);
                            cx.notify();
                        }
                        Err(err) => log::error!("Failed to fetch rows at {page}: {err}"),
                    }
                });
            })
            .detach();
        }
    }

    fn evict_distant_pages(&mut self) {
        if self
            .source
            .as_ref()
            .is_some_and(|source| source.loading_mode == LoadingMode::Eager)
        {
            return;
        }
        let visible_pages = self
            .visible_end
            .saturating_sub(self.visible_start)
            .div_ceil(PAGE_SIZE)
            + 1;
        while self.data.len() > MAX_CACHED_PAGES.max(visible_pages + 2) {
            let farthest = self
                .data
                .keys()
                .copied()
                .max_by_key(|page| page.abs_diff(self.visible_start / PAGE_SIZE * PAGE_SIZE));
            if let Some(page) = farthest {
                self.data.remove(&page);
            }
        }
    }

    fn on_filter_input_event(
        &mut self,
        _state: &Entity<InputState>,
        event: &InputEvent,
        cx: &mut Context<TableState<Self>>,
    ) {
        match event {
            InputEvent::Change => {
                self.filter_data(cx);
            }
            _ => {}
        };
    }

    fn create_column_info(&mut self) {
        let previous_widths: HashMap<_, _> = self
            .columns
            .iter()
            .map(|column| (column.key.clone(), column.width))
            .collect();
        let Some(source) = &self.source else {
            return;
        };
        let data_columns: Vec<_> = source
            .columns
            .iter()
            .enumerate()
            .map(|(col_ix, column_info)| {
                let name = SharedString::new(column_info.name.as_str());
                let dtype = column_info.kind;
                let (min_width, preferred_width, max_width) = column_widths(
                    name.as_ref(),
                    dtype,
                    self.data
                        .get(&0)
                        .into_iter()
                        .flatten()
                        .take(100)
                        .filter_map(|row| row[col_ix].clone()),
                );
                let width = previous_widths
                    .get(&name)
                    .copied()
                    .unwrap_or_else(|| px(preferred_width));
                let mut column = Column::new(name.clone(), name)
                    .sortable()
                    .width(width)
                    .min_width(px(min_width))
                    .max_width(px(max_width));

                if matches!(dtype, ColumnKind::Number | ColumnKind::Float) {
                    column = column.text_right();
                } else if dtype == ColumnKind::Boolean {
                    column = column.text_center();
                }

                column
            })
            .collect();
        self.columns = vec![
            Column::new("__tabulite_row_number", "#")
                .fixed_left()
                .movable(false)
                .resizable(false)
                .selectable(false)
                .p_0()
                .width(row_number_width(self.row_count)),
        ];
        self.columns.extend(data_columns);

        self.input_subscriptions.clear();
        self.filter_inputs.clear();
        self.any_filter_input = None;
    }
}

impl TableDelegate for TableLayer {
    fn loading(&self, _cx: &App) -> bool {
        self.columns.is_empty()
    }

    fn columns_count(&self, _: &App) -> usize {
        debug_assert!(
            self.data
                .values()
                .flatten()
                .all(|row| row.len() + 1 == self.columns.len())
        );
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.row_count
    }

    fn visible_rows_changed(
        &mut self,
        visible_range: Range<usize>,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        self.request_range(visible_range, cx);
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        self.columns[col_ix].clone()
    }

    fn render_header(
        &mut self,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let div = div().id("header");
        if self.filter_enabled {
            self.ensure_filter_inputs(window, cx);
        }

        div
    }

    fn group_headers(&self, _: &App) -> Option<Vec<Vec<ColumnGroup>>> {
        self.filter_enabled.then(|| {
            vec![
                self.columns
                    .iter()
                    .map(|column| ColumnGroup::new(column.name.clone(), 1))
                    .collect(),
            ]
        })
    }

    fn render_th(
        &mut self,
        col_ix: usize,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        if self.filter_enabled && col_ix > 0 {
            let table_focus_handle = self.table_focus_handle.clone();
            let enter_focus_handle = table_focus_handle.clone();
            let table = cx.entity();
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .child(
                    Input::new(
                        &self
                            .filter_inputs
                            .get(col_ix - 1)
                            .expect("BUG: column index"),
                    )
                    .w_full()
                    .prefix(Icon::new(IconName::Search))
                    .text_xs()
                    .xsmall(),
                )
                .on_action(move |_: &Enter, window, cx| {
                    if let Some(focus_handle) = &enter_focus_handle {
                        focus_handle.focus(window, cx);
                    } else {
                        window.blur(cx);
                    }
                })
                .on_action(move |_: &Escape, window, cx| {
                    table.update(cx, |table, cx| {
                        if table.delegate_mut().hide_filters_if_empty(cx) {
                            table.refresh_header_layout(cx);
                        }
                    });
                    if let Some(focus_handle) = &table_focus_handle {
                        focus_handle.focus(window, cx);
                    } else {
                        window.blur(cx);
                    }
                })
        } else {
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .truncate()
                .when(col_ix == 0, |this| this.flex().items_center().justify_end())
                .when(!self.filter_enabled || col_ix != 0, |this| {
                    this.child(self.column(col_ix, cx).name.clone())
                })
        }
    }

    fn render_tr(
        &mut self,
        row_ix: usize,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        div().id(("row", row_ix)).when(row_ix % 2 != 0, |this| {
            this.bg(cx.theme().tokens.table_even)
        })
    }

    fn render_group_th(
        &mut self,
        label: &SharedString,
        _col_span: usize,
        width: Pixels,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let is_selected = self
            .selected_col
            .and_then(|col_ix| self.columns.get(col_ix))
            .is_some_and(|column| &column.name == label);

        div()
            .w(width)
            .h_full()
            .flex_shrink_0()
            .flex()
            .items_center()
            .table_cell_size(Size::XSmall)
            .border_r_1()
            .border_color(cx.theme().border)
            .when(is_selected, |this| {
                this.bg(cx.theme().tokens.table_active)
                    .text_color(cx.theme().foreground)
            })
            .child(
                div()
                    .w_full()
                    .truncate()
                    .when(label == &self.columns[0].name, |this| this.text_right())
                    .child(label.clone()),
            )
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<'_, TableState<Self>>,
    ) -> impl IntoElement {
        if col_ix == 0 {
            return div()
                .flex()
                .items_center()
                .justify_end()
                .size_full()
                .pr(px(8.))
                .bg(cx.theme().tokens.table_head)
                .text_color(cx.theme().table_head_foreground)
                .child(row_number(row_ix))
                .into_any_element();
        }
        let data_ix = col_ix - 1;
        if let Some(badge) = self.cell_badge(row_ix, data_ix) {
            return div()
                .flex()
                .justify_center()
                .child(
                    Tag::secondary()
                        .outline()
                        .xsmall()
                        .child(SharedString::new(badge)),
                )
                .text_color(cx.theme().accent)
                .into_any_element();
        }
        match self.cell(row_ix, data_ix) {
            None | Some(None) => div().into_any_element(),
            Some(Some(value)) => {
                let align = self.columns[col_ix].align;
                let table = cx.entity();
                let generation = self.generation();
                let value = value.to_owned();
                div()
                    .id(("filter-cell", row_ix * self.columns.len() + col_ix))
                    .flex()
                    .items_center()
                    .size_full()
                    .truncate()
                    .when(align == TextAlign::Center, |this| this.justify_center())
                    .when(align == TextAlign::Right, |this| this.justify_end())
                    .child(SharedString::new(value.clone()))
                    .context_menu(move |menu, _, _| {
                        cell_filter_menu(menu, table.clone(), col_ix, value.clone(), generation)
                    })
                    .into_any_element()
            }
        }
    }

    fn cell_text(&self, row_ix: usize, col_ix: usize, _: &App) -> String {
        if col_ix == 0 {
            return row_number(row_ix);
        }
        self.cell(row_ix, col_ix - 1)
            .flatten()
            .unwrap_or_default()
            .to_string()
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        if col_ix == 0 {
            return;
        }
        self.sort = match sort {
            ColumnSort::Ascending => Some((col_ix - 1, false)),
            ColumnSort::Descending => Some((col_ix - 1, true)),
            ColumnSort::Default => None,
        };
        for (ix, column) in self.columns.iter_mut().enumerate() {
            column.sort = Some(if ix == col_ix {
                sort
            } else {
                ColumnSort::Default
            });
        }
        self.filter_data(cx);
    }
}

fn badge_label(kind: ColumnKind, value: Option<&str>) -> Option<String> {
    match value {
        None => Some(NULL.to_owned()),
        Some(size) if kind == ColumnKind::Blob => Some(format!("blob ({size})")),
        Some(_) => None,
    }
}

fn column_widths(
    name: &str,
    dtype: ColumnKind,
    values: impl Iterator<Item = String>,
) -> (f32, f32, f32) {
    let (min_width, max_width) = if dtype == ColumnKind::Boolean {
        (80.0, 140.0)
    } else if matches!(dtype, ColumnKind::Number | ColumnKind::Float) {
        (90.0, 240.0)
    } else if dtype == ColumnKind::Temporal {
        (130.0, 280.0)
    } else if dtype == ColumnKind::Text {
        (120.0, 480.0)
    } else {
        (110.0, 360.0)
    };
    let max_chars = values
        .map(|value| value.chars().count())
        .fold(name.chars().count(), usize::max)
        .min(64);
    let preferred_width = (max_chars as f32 * 7.5 + 40.0).clamp(min_width, max_width);

    (min_width, preferred_width, max_width)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn keeps_only_a_bounded_page_of_a_large_file() {
        let path =
            std::env::temp_dir().join(format!("tabulite-page-cache-{}.csv", std::process::id()));
        let mut csv = String::from("name,amount\n");
        for ix in 0..1024 {
            csv.push_str(&format!("row_{ix},{ix}\n"));
        }
        std::fs::write(&path, csv).unwrap();
        let result = (|| -> anyhow::Result<()> {
            let source = crate::tableio::layer_data(&path, "page-cache")?;
            let count = source.count(&[])?;
            let page = source.query_page(&[], None, 0, PAGE_SIZE)?;
            let mut layer = TableLayer::default();
            layer.update_data(source, count, page);
            assert_eq!(layer.rows_count(), 1024);
            assert_eq!(layer.columns_count(), 3);
            assert_eq!(layer.columns[0].name.as_ref(), "#");
            assert_eq!(layer.columns[1].name.as_ref(), "name");
            assert!(!layer.columns[0].selectable);
            assert!(layer.columns[0].paddings.is_some());
            assert_eq!(layer.columns[0].width, row_number_width(1024));
            assert_eq!(layer.data.len(), 1);
            assert_eq!(layer.data[&0].len(), PAGE_SIZE);
            assert_eq!(layer.cell(0, 0), Some(Some("row_0")));
            assert_eq!(layer.cell(PAGE_SIZE - 1, 0), Some(Some("row_127")));
            assert_eq!(layer.cell(PAGE_SIZE, 0), None);
            layer.set_row_count(1_000_000);
            assert_eq!(layer.columns[0].width, row_number_width(1_000_000));
            Ok(())
        })();
        std::fs::remove_file(path).unwrap();
        result.unwrap();
    }

    #[test]
    fn eager_tables_keep_every_row_across_page_boundaries() {
        let path = std::env::temp_dir().join(format!("tabulite-eager-{}.csv", std::process::id()));
        let mut csv = String::from("name,amount\n");
        for ix in 0..300 {
            csv.push_str(&format!("row_{ix},{ix}\n"));
        }
        std::fs::write(&path, csv).unwrap();
        let result = (|| -> anyhow::Result<()> {
            let mut source = crate::tableio::layer_data(&path, "eager")?;
            source.loading_mode = LoadingMode::Eager;
            let rows = source.query_all(&[], None)?;
            let mut layer = TableLayer::default();
            layer.update_data(source, rows.rows.len(), rows);
            assert_eq!(layer.rows_count(), 300);
            assert_eq!(layer.cell(0, 0), Some(Some("row_0")));
            assert_eq!(layer.cell(128, 0), Some(Some("row_128")));
            assert_eq!(layer.cell(299, 0), Some(Some("row_299")));
            layer.evict_distant_pages();
            assert_eq!(layer.data.len(), 3);
            Ok(())
        })();
        std::fs::remove_file(path).unwrap();
        result.unwrap();
    }

    #[test]
    fn evicts_pages_outside_the_visible_range() {
        let mut layer = TableLayer::default();
        layer.visible_start = 9 * PAGE_SIZE;
        layer.visible_end = 10 * PAGE_SIZE;
        for page in 0..10 {
            layer.data.insert(page * PAGE_SIZE, vec![vec![None]]);
        }
        layer.evict_distant_pages();
        assert!(layer.data.len() <= MAX_CACHED_PAGES);
        assert!(!layer.data.contains_key(&0));
        assert!(layer.data.contains_key(&(9 * PAGE_SIZE)));
    }

    #[test]
    fn row_numbers_use_display_positions_across_pages() {
        assert_eq!(row_number(0), "1");
        assert_eq!(row_number(PAGE_SIZE), "129");
        assert_eq!(row_number(1_000_000), "1000001");
        assert_eq!(row_number_width(0), px(40.0));
        assert_eq!(row_number_width(129), px(48.0));
        assert_eq!(row_number_width(1_000_000), px(80.0));
        assert_eq!(row_number_width(1_000_000_000), px(104.0));
    }

    #[test]
    fn badges_distinguish_blobs_from_null_and_text() {
        assert_eq!(
            badge_label(ColumnKind::Blob, Some("3 B")),
            Some("blob (3 B)".into())
        );
        assert_eq!(
            badge_label(ColumnKind::Blob, Some("1.5 MB")),
            Some("blob (1.5 MB)".into())
        );
        assert_eq!(badge_label(ColumnKind::Blob, None), Some("null".into()));
        assert_eq!(badge_label(ColumnKind::Text, None), Some("null".into()));
        assert_eq!(badge_label(ColumnKind::Text, Some("3 B")), None);
    }

    #[test]
    fn constrains_inferred_column_widths() {
        assert_eq!(
            column_widths("enabled", ColumnKind::Boolean, [].into_iter()),
            (80.0, 92.5, 140.0)
        );
        assert_eq!(
            column_widths("name", ColumnKind::Text, ["x".repeat(200)].into_iter()),
            (120.0, 480.0, 480.0)
        );
    }
}
