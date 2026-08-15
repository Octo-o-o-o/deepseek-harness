//! Desktop notifications for moments the user is not looking at.
//!
//! The page reports the fact ("this session is waiting for approval"); the
//! shell decides whether that fact deserves a notification. Window visibility
//! and focus are the shell's knowledge, and a notification fired while the user
//! is already looking at the answer is noise, not help.
//!
//! Only the shell surface has this: in a browser tab `window.__TAURI__` is
//! absent, the renderer's call is skipped, and nothing here runs.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

/// Window used to collapse repeats of the same session's request.
///
/// A pending approval re-renders whenever the conversation view updates, so the
/// renderer can report the same wait many times; without this the user would
/// get a burst of identical banners for one decision.
const REPEAT_WINDOW: Duration = Duration::from_secs(60);

/// Last time each session raised a notification, for {@link REPEAT_WINDOW}.
#[derive(Default)]
pub struct AttentionState {
    last: Mutex<HashMap<String, Instant>>,
}

impl AttentionState {
    /// Create the empty de-duplication table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether this session may raise a notification now.
    fn admit(&self, session: &str) -> bool {
        let mut last = self.last.lock().expect("attention mutex");
        let now = Instant::now();
        match last.get(session) {
            Some(previous) if now.duration_since(*previous) < REPEAT_WINDOW => false,
            _ => {
                last.insert(session.to_string(), now);
                true
            }
        }
    }
}

/// Whether the user can already see the window, in which case a notification
/// would duplicate what is on screen.
fn user_is_watching(app: &AppHandle) -> bool {
    let Some(window) = app.get_webview_window("main") else {
        return false;
    };
    let visible = window.is_visible().unwrap_or(false);
    let minimized = window.is_minimized().unwrap_or(false);
    let focused = window.is_focused().unwrap_or(false);
    visible && !minimized && focused
}

/// Raise a desktop notification for a session that needs the user.
///
/// Called by the renderer through `invoke`. The shell drops the request when
/// the window is already in front, or when the same session asked within
/// {@link REPEAT_WINDOW}.
///
/// # Parameters
/// - `app`: Tauri app handle.
/// - `state`: de-duplication table.
/// - `session`: session id the wait belongs to; also the de-duplication key.
/// - `title`: short notification title.
/// - `body`: one-line detail.
#[tauri::command]
pub fn notify_attention(
    app: AppHandle,
    state: tauri::State<'_, AttentionState>,
    session: String,
    title: String,
    body: String,
) {
    if user_is_watching(&app) {
        return;
    }
    if !state.admit(&session) {
        return;
    }
    if let Err(error) = app.notification().builder().title(title).body(body).show() {
        // A denied or unavailable notification permission must not break the
        // turn the renderer is in the middle of reporting.
        eprintln!("desktop: notification failed: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeats_within_the_window_are_dropped_per_session() {
        let state = AttentionState::new();
        assert!(state.admit("a"));
        // The same session re-reporting the same wait is collapsed.
        assert!(!state.admit("a"));
        // A different session is an independent decision.
        assert!(state.admit("b"));
        assert!(!state.admit("b"));
    }

    #[test]
    fn an_elapsed_window_admits_the_session_again() {
        let state = AttentionState::new();
        assert!(state.admit("a"));
        state.last.lock().expect("attention mutex").insert(
            "a".into(),
            Instant::now() - REPEAT_WINDOW - Duration::from_secs(1),
        );
        assert!(state.admit("a"));
    }
}
