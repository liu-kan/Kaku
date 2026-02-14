#[cfg(target_os = "macos")]
mod macos;

#[derive(Debug, Clone)]
pub struct ToastNotification {
    pub title: String,
    pub message: String,
    pub url: Option<String>,
    pub timeout: Option<std::time::Duration>,
}

impl ToastNotification {
    pub fn show(self) {
        show(self)
    }
}

pub fn show(notif: ToastNotification) {
    #[cfg(target_os = "macos")]
    if let Err(err) = macos::show_notif(notif) {
        log::error!("Failed to show notification: {}", err);
    }
    #[cfg(not(target_os = "macos"))]
    {
        log::debug!(
            "Notification (no-op backend): title={:?}, message={:?}, url={:?}, timeout={:?}",
            notif.title,
            notif.message,
            notif.url,
            notif.timeout
        );
    }
}

pub fn persistent_toast_notification_with_click_to_open_url(title: &str, message: &str, url: &str) {
    show(ToastNotification {
        title: title.to_string(),
        message: message.to_string(),
        url: Some(url.to_string()),
        timeout: None,
    });
}

pub fn persistent_toast_notification(title: &str, message: &str) {
    show(ToastNotification {
        title: title.to_string(),
        message: message.to_string(),
        url: None,
        timeout: None,
    });
}

#[cfg(target_os = "macos")]
pub use macos::initialize as macos_initialize;
