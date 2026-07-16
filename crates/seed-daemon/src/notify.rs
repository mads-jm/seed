/// Notifier trait + desktop and mock implementations.
///
/// `DesktopNotifier` uses `notify-rust` via `spawn_blocking`. It attaches a
/// freedesktop "Log" action to every reminder notification; when the user taps
/// or clicks Log, the daemon receives an `Action::Complete` through the
/// command channel and the resulting `ReminderCompleted` event flows out to
/// every subscriber (TUI, bridge, future clients) on the regular diff stream.
///
/// `MockNotifier` records calls for tests without firing OS notifications.
use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;
use seed_core::ReminderId;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::ipc::Command;
use seed_wire::Action;

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

pub struct DesktopNotifier {
    /// Channel back into the daemon's action loop. When a notification's
    /// "Log" action is invoked, the notifier synthesises a no-response
    /// `Command::Action { Action::Complete }` and sends it here.
    ///
    /// `None` in tests / environments where action wiring isn't required —
    /// the notification still fires, the Log button just no-ops.
    cmd_tx: Option<mpsc::Sender<Command>>,
}

impl DesktopNotifier {
    /// New notifier with action wiring. Pass the daemon's command channel so
    /// invoked actions can commit events through the existing pipeline.
    pub fn new(cmd_tx: mpsc::Sender<Command>) -> Self {
        Self {
            cmd_tx: Some(cmd_tx),
        }
    }

    /// New notifier without action wiring — fires notifications but ignores
    /// the Log button. Useful in tests and CLI smoke runs.
    #[allow(dead_code)]
    pub fn detached() -> Self {
        Self { cmd_tx: None }
    }
}

#[async_trait]
impl Notifier for DesktopNotifier {
    async fn notify(&self, reminder_id: &ReminderId, title: &str, body: &str) -> Result<()> {
        let title = title.to_owned();
        let body = body.to_owned();
        let cmd_tx = self.cmd_tx.clone();
        let rid = reminder_id.clone();

        // Fire-and-forget. `wait_for_action` blocks its OS thread until the
        // toast is dismissed or actioned — potentially indefinitely for a
        // persistent/manual-dismiss server. This runs inside `spawn_blocking`,
        // but we must NOT `.await` the join handle here: `notify()` is awaited
        // on the daemon's main command loop (Daemon::run_scheduler_tick, some
        // call sites under the `state` write lock), so awaiting completion
        // would stall the loop — clients connect, get `Hello`, then never
        // receive their `Snapshot`/`StateDiff` until the toast clears. Detach
        // the blocking task instead and return immediately; the action
        // callback owns a `cmd_tx` clone and routes `Action::Complete` back
        // through the daemon independently. (This matches the original intent
        // noted below: many toasts may pend concurrently, each on its own
        // blocking-pool thread — tokio's default pool (~512) dwarfs the
        // handful of overlapping reminders.)
        tokio::task::spawn_blocking(move || {
            let handle = match notify_rust::Notification::new()
                .summary(&title)
                .body(&body)
                // "default" is the freedesktop convention for the action that
                // fires on plain tap/click — no extra button rendered, the
                // whole toast is the affordance. The label is shown by
                // notification servers that surface a default-action label
                // (most don't); it's harmless either way.
                .action("default", "Log")
                .show()
            {
                Ok(h) => h,
                Err(e) => {
                    // Non-fatal — log and drop the toast.
                    warn!("desktop notification failed: {e}");
                    return;
                }
            };

            handle.wait_for_action(|action_key| {
                if action_key == "default" {
                    debug!(reminder = %rid.0, "notification action 'default' invoked");
                    if let Some(tx) = &cmd_tx {
                        // Throwaway response channel — request_id=0 means
                        // "no response expected" (matches the internal tick
                        // pattern in Daemon::handle_action).
                        let (resp_tx, _resp_rx) = mpsc::channel(1);
                        let cmd = Command::Action {
                            conn_id: 0,
                            request_id: 0,
                            action: Action::Complete {
                                reminder_id: rid.clone(),
                            },
                            resp_tx,
                        };
                        if let Err(e) = tx.blocking_send(cmd) {
                            warn!("notification action send failed: {e}");
                        }
                    }
                }
            });
        });

        Ok(())
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
