/// Notifier trait + desktop and mock implementations.
///
/// `DesktopNotifier` uses `notify-rust` via `spawn_blocking`.
/// `MockNotifier` records calls for tests without firing OS notifications.
use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;
use seed_core::ReminderId;
use tracing::warn;

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

#[async_trait]
pub trait Notifier: Send + Sync {
    /// Fire a notification for the given reminder with the provided body text.
    async fn notify(&self, reminder_id: &ReminderId, title: &str, body: &str) -> Result<()>;
}

// ---------------------------------------------------------------------------
// Desktop implementation
// ---------------------------------------------------------------------------

pub struct DesktopNotifier;

#[async_trait]
impl Notifier for DesktopNotifier {
    async fn notify(&self, _reminder_id: &ReminderId, title: &str, body: &str) -> Result<()> {
        let title = title.to_owned();
        let body = body.to_owned();
        let result = tokio::task::spawn_blocking(move || {
            notify_rust::Notification::new()
                .summary(&title)
                .body(&body)
                .show()
        })
        .await;

        match result {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => {
                warn!("desktop notification failed: {e}");
                Ok(()) // Non-fatal — log and continue.
            }
            Err(e) => {
                warn!("spawn_blocking for notification panicked: {e}");
                Ok(())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Mock implementation (test double)
// ---------------------------------------------------------------------------

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Default)]
pub struct MockNotifier {
    /// Records (reminder_id, title, body) for each call.
    pub calls: Arc<Mutex<Vec<(String, String, String)>>>,
}

impl MockNotifier {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn was_called_for(&self, reminder_id: &str) -> bool {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .any(|(id, _, _)| id == reminder_id)
    }
}

#[async_trait]
impl Notifier for MockNotifier {
    async fn notify(&self, reminder_id: &ReminderId, title: &str, body: &str) -> Result<()> {
        self.calls
            .lock()
            .unwrap()
            .push((reminder_id.0.clone(), title.to_owned(), body.to_owned()));
        Ok(())
    }
}
