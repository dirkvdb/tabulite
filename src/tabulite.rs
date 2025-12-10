use gpui::*;
use gpui::{App, IntoElement, Window};
use gpui_component::*;
use std::path::PathBuf;

use crate::tableview::TableView;

actions!(story, [Open, Quit, ToggleFilter,]);

pub struct Tabulite {
    table: Entity<TableView>,
}

impl Tabulite {
    pub fn view(path: Option<PathBuf>, window: &mut Window, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self::new(path, window, cx))
    }

    fn new(path: Option<PathBuf>, window: &mut Window, cx: &mut gpui::Context<Self>) -> Self {
        let table = TableView::view(path, window, cx);

        Self { table }
    }
}

impl Render for Tabulite {
    fn render(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let notification_layer = Root::render_notification_layer(window, cx);

        div()
            .v_flex()
            .size_full()
            .child(self.table.clone())
            .children(notification_layer)
    }
}
