use gpui_kit::component::*;
use gpui_kit::*;
use std::path::PathBuf;

use crate::tableview::TableView;
use crate::utils;

actions!(
    story,
    [
        ClearFilter,
        DismissFilters,
        Open,
        Quit,
        ScrollHalfPageDown,
        ScrollHalfPageUp,
        SelectFirstDataColumn,
        SelectFirstRow,
        SelectLastRow,
        SelectNextColumn,
        SelectNextLayer,
        SelectNextRow,
        SelectPreviousColumn,
        SelectPreviousLayer,
        SelectPreviousRow,
        ToggleAnyFilter,
        ToggleFilter,
    ]
);

pub struct Tabulite {
    table: Entity<TableView>,
}

impl Tabulite {
    pub fn view(path: Option<PathBuf>, window: &mut Window, cx: &mut App) -> Entity<Self> {
        let view = cx.new(|cx| Self::new(path, window, cx));
        let weak_view = view.downgrade();
        cx.on_action(move |_: &Open, cx| {
            log::warn!("Open shortcut received");
            let _ = weak_view.update(cx, |view, cx| view.open_file(cx));
        });
        let weak_view = view.downgrade();
        cx.on_action(move |_: &SelectFirstDataColumn, cx| {
            let _ = weak_view.update(cx, |view, cx| {
                view.table.update(cx, TableView::select_first_data_column);
            });
        });
        let weak_view = view.downgrade();
        cx.on_action(move |_: &SelectPreviousColumn, cx| {
            let _ = weak_view.update(cx, |view, cx| {
                view.table.update(cx, TableView::select_previous_column);
            });
        });
        let weak_view = view.downgrade();
        cx.on_action(move |_: &SelectNextColumn, cx| {
            let _ = weak_view.update(cx, |view, cx| {
                view.table.update(cx, TableView::select_next_column);
            });
        });
        let weak_view = view.downgrade();
        cx.on_action(move |_: &SelectPreviousLayer, cx| {
            let _ = weak_view.update(cx, |view, cx| {
                view.table.update(cx, TableView::select_previous_layer);
            });
        });
        let weak_view = view.downgrade();
        cx.on_action(move |_: &SelectNextLayer, cx| {
            let _ = weak_view.update(cx, |view, cx| {
                view.table.update(cx, TableView::select_next_layer);
            });
        });
        let weak_view = view.downgrade();
        cx.on_action(move |_: &SelectPreviousRow, cx| {
            let _ = weak_view.update(cx, |view, cx| {
                view.table.update(cx, TableView::select_previous_row);
            });
        });
        let weak_view = view.downgrade();
        cx.on_action(move |_: &SelectNextRow, cx| {
            let _ = weak_view.update(cx, |view, cx| {
                view.table.update(cx, TableView::select_next_row);
            });
        });
        let weak_view = view.downgrade();
        cx.on_action(move |_: &ScrollHalfPageDown, cx| {
            let _ = weak_view.update(cx, |view, cx| {
                view.table
                    .update(cx, |table, cx| table.scroll_half_page(true, cx));
            });
        });
        let weak_view = view.downgrade();
        cx.on_action(move |_: &ScrollHalfPageUp, cx| {
            let _ = weak_view.update(cx, |view, cx| {
                view.table
                    .update(cx, |table, cx| table.scroll_half_page(false, cx));
            });
        });
        let weak_view = view.downgrade();
        cx.on_action(move |_: &SelectFirstRow, cx| {
            let _ = weak_view.update(cx, |view, cx| {
                view.table.update(cx, TableView::select_first_row);
            });
        });
        let weak_view = view.downgrade();
        cx.on_action(move |_: &SelectLastRow, cx| {
            let _ = weak_view.update(cx, |view, cx| {
                view.table.update(cx, TableView::select_last_row);
            });
        });
        view
    }

    fn new(path: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let table = TableView::view(path, window, cx);

        Self { table }
    }

    fn open_file(&mut self, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Open".into()),
        });
        let table = self.table.clone();

        cx.spawn(async move |_, cx| match prompt.await {
            Ok(Ok(Some(paths))) => {
                if let Some(path) = paths.into_iter().next() {
                    let _ = table.update(cx, |view, cx| {
                        view.load_table(path, cx).detach();
                    });
                }
            }
            Ok(Ok(None)) | Err(_) => {}
            Ok(Err(err)) => utils::error_notification("Failed to open file picker", err, cx),
        })
        .detach();
    }
}

impl Render for Tabulite {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().v_flex().size_full().child(self.table.clone())
    }
}
