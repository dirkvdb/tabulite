use gpui::{App, Entity, Menu, MenuItem, SharedString, Window};
use gpui_component::menu::AppMenuBar;

use crate::{Open, Quit, tabulite::ToggleFilter};

pub fn init(
    title: impl Into<SharedString>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<AppMenuBar> {
    let app_menu_bar = AppMenuBar::new(window, cx);
    let title: SharedString = title.into();
    update_app_menu(title.clone(), app_menu_bar.clone(), cx);

    app_menu_bar
}

fn update_app_menu(
    title: impl Into<SharedString>,
    _app_menu_bar: Entity<AppMenuBar>,
    cx: &mut App,
) {
    cx.set_menus(vec![
        Menu {
            name: title.into(),
            items: vec![
                MenuItem::action("Open...", Open),
                MenuItem::Separator,
                MenuItem::action("Quit", Quit),
            ],
        },
        Menu {
            name: "Window".into(),
            items: vec![MenuItem::action("Toggle Search", ToggleFilter)],
        },
    ]);
}
