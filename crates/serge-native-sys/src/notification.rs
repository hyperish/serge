use std::io;

/// A stable command ID and its localized notification button title.
#[derive(Clone, Copy, Debug)]
pub struct NotificationAction<'a> {
    /// Application command to deliver when selected.
    pub id: &'a str,
    /// Localized text shown on the button.
    pub title: &'a str,
}
/// A shared group of actions used by local notifications and Apple push categories.
#[derive(Clone, Copy, Debug)]
pub struct NotificationCategory<'a> {
    /// Stable category ID.
    pub id: &'a str,
    /// Up to three actions in display order.
    pub actions: &'a [NotificationAction<'a>],
}
/// A concrete user selection. Read the current application data before acting on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotificationResponse {
    /// ID supplied when posting the notification.
    pub notification: String,
    /// Selected command ID.
    pub action: String,
}
pub(crate) fn valid_id(id: &str) -> bool {
    (1..=160).contains(&id.len())
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
}
impl NotificationCategory<'_> {
    pub(crate) fn validate(&self) -> io::Result<()> {
        if !valid_id(self.id)
            || self.actions.len() > 3
            || self.actions.iter().enumerate().any(|(i, a)| {
                !valid_id(a.id)
                    || a.title.trim().is_empty()
                    || a.title.len() > 512
                    || self.actions[..i].iter().any(|b| b.id == a.id)
            })
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid notification category or action",
            ));
        }
        Ok(())
    }
}
pub(crate) fn validate_message(id: &str, title: &str, body: &str) -> io::Result<()> {
    if !valid_id(id) || title.trim().is_empty() || title.len() > 1024 || body.len() > 8192 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid notification ID, title or text",
        ));
    }
    Ok(())
}
