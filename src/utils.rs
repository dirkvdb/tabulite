use anyhow::Error;
use gpui::{AppContext as _, AsyncApp, SharedString};
use gpui_component::{WindowExt as _, notification::Notification};

pub fn error_message(context: &str, err: Error) -> SharedString {
    let err_msg = if let Some(io_err) = err
        .downcast_ref::<std::io::Error>()
        .or_else(|| err.root_cause().downcast_ref::<std::io::Error>())
    {
        match io_err.kind() {
            std::io::ErrorKind::NotFound => "File not found.".into(),
            std::io::ErrorKind::PermissionDenied => "File not found.".into(),
            _ => SharedString::from(format!("{context}\n{err}")),
        }
    } else {
        err.to_string().into()
    };

    SharedString::from(format!("{context}\n{err_msg}"))
}

pub fn error_notification(context: &str, err: Error, cx: &mut AsyncApp) {
    let message = error_message(context, err);

    let _ = cx.update(|app| {
        if let Some(window_handle) = app.active_window() {
            let _ = app.update_window(window_handle, |_, window, app| {
                window.push_notification(Notification::error(message), app);
            });
        }
    });
}
