//! Single source of status/error text styling: setup validation errors,
//! solve-worker failure/cancel messages, Results load errors, and
//! node-evaluation failure text all render through [`show`] instead of each
//! picking its own color and (in)consistent wording.

use eframe::egui;
use egui::{Color32, Ui};

use crate::theme;

/// Severity of one status line. `Info` covers confirmations ("saved",
/// "OK -- ready to solve") and in-progress notices; `Warning` covers
/// recoverable/advisory conditions (a dependent control that will be
/// ignored, "confirm delete?"); `Error` covers a request that failed
/// outright (validation errors, worker/evaluation failures, load errors).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Warning,
    Error,
}

/// This level's text color.
pub fn color(level: Level) -> Color32 {
    match level {
        Level::Info => theme::ACCENT,
        Level::Warning => theme::WARNING,
        Level::Error => theme::ERROR,
    }
}

/// This level's icon prefix, or `""` for `Info` (a confirmation reads fine
/// unadorned; a warning or error should visually stand out from the rest of
/// the info-dense form even at a glance).
pub fn icon(level: Level) -> &'static str {
    match level {
        Level::Info => "",
        Level::Warning | Level::Error => "\u{26a0}",
    }
}

/// `message` prefixed with this level's icon (if any), e.g.
/// `"\u{26a0} Vector traverser requires ..."`.
pub fn text(level: Level, message: impl AsRef<str>) -> String {
    let icon = icon(level);
    if icon.is_empty() {
        message.as_ref().to_string()
    } else {
        format!("{icon} {}", message.as_ref())
    }
}

/// Draws one status/error line at `level`, colored and icon-prefixed
/// consistently with every other status line in the GUI.
pub fn show(ui: &mut Ui, level: Level, message: impl AsRef<str>) {
    ui.colored_label(color(level), text(level, message));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_has_no_icon() {
        assert_eq!(icon(Level::Info), "");
        assert_eq!(text(Level::Info, "OK"), "OK");
    }

    #[test]
    fn warning_and_error_are_icon_prefixed() {
        assert_eq!(text(Level::Warning, "careful"), "\u{26a0} careful");
        assert_eq!(text(Level::Error, "broken"), "\u{26a0} broken");
    }

    #[test]
    fn warning_and_error_colors_differ() {
        assert_ne!(color(Level::Warning), color(Level::Error));
        assert_ne!(color(Level::Info), color(Level::Error));
    }
}
