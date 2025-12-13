use gpui::{App, KeyBinding};

use crate::tabulite::Open;
use crate::tabulite::Quit;
use crate::tabulite::ToggleFilter;

pub mod appconfig;
mod tableio;
mod tablelayer;
mod tableview;
pub mod tabulite;
mod titlebar;
mod utils;
mod appmenus;

pub fn init(cx: &mut App) {
    gpui_component::init(cx);

    cx.bind_keys([
        KeyBinding::new("/", ToggleFilter, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-o", Open, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-o", Open, None),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-q", Quit, None),
        #[cfg(target_os = "windows")]
        KeyBinding::new("alt-f4", Quit, None),
        #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
        KeyBinding::new("ctrl-q", Quit, None),
    ]);

    cx.on_action(|_: &Quit, cx: &mut App| {
        cx.quit();
    });

    cx.activate(true);
}
