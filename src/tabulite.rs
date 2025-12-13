use gpui::*;
use gpui::{App, IntoElement, Window};
use gpui_component::*;
use std::path::PathBuf;

use crate::tableview::TableView;
use crate::titlebar::AppTitleBar;

actions!(story, [Open, Quit, ToggleFilter,]);

pub struct Tabulite {
    table: Entity<TableView>,
    title_bar: Entity<AppTitleBar>,
}

impl Tabulite {
    pub fn view(path: Option<PathBuf>, window: &mut Window, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self::new(path, window, cx))
    }

    fn new(path: Option<PathBuf>, window: &mut Window, cx: &mut gpui::Context<Self>) -> Self {
        let title_bar = cx.new(|cx| AppTitleBar::new("tabulite", window, cx));
        let table = TableView::view(path, window, cx);

        Self { table, title_bar }
    }
}

impl Render for Tabulite {
    fn render(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let notification_layer = Root::render_notification_layer(window, cx);

        div()
            .v_flex()
            .size_full()
            .child(self.title_bar.clone())
            .child(self.table.clone())
            .children(notification_layer)
    }
}
