use gpui_kit::component::*;
use gpui_kit::*;
use std::path::PathBuf;

use crate::tableview::TableView;
use crate::utils;

actions!(
    story,
    [
        Open,
        Quit,
        SelectNextColumn,
        SelectPreviousColumn,
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
                    let _ = table.update(cx, |_, cx| {
                        TableView::load_table(path, cx).detach();
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let notification_layer = Root::render_notification_layer(window, cx);

        div()
            .v_flex()
            .size_full()
            .child(self.table.clone())
            .children(notification_layer)
    }
}
