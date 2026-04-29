/// Append-only event log backed by `events.jsonl`.
///
/// Each line is a JSON-serialised `EventEnvelope`. The file is fsynced after
/// every write for durability. Malformed lines are skipped with a warning on
/// load (power-cut resilience).
use std::{
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use chrono::Utc;
use seed_core::{Event, State, from_envelope, to_envelope};
use serde::{Deserialize, Serialize};
use tracing::warn;

// ---------------------------------------------------------------------------
// Snapshot on-disk format
// ---------------------------------------------------------------------------

/// Persisted snapshot: state + number of events already consumed when the
/// snapshot was taken. The daemon skips that many JSONL lines on startup.
#[derive(Serialize, Deserialize)]
struct SnapshotFile {
    event_count: usize,
    state: State,
}

// ---------------------------------------------------------------------------
// EventLog
// ---------------------------------------------------------------------------

pub struct EventLog {
    #[allow(dead_code)]
    path: PathBuf,
    file: File,
    /// Running count of events written to the file (in this session + prior).
    pub event_count: usize,
}

impl EventLog {
    /// Open (or create) the event log file for appending.
    pub fn open(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("failed to open event log: {}", path.display()))?;

        // Count existing lines to initialise `event_count`.
        let existing = count_lines(path)?;

        Ok(Self {
            path: path.to_owned(),
            file,
            event_count: existing,
        })
    }

    /// Append one event to the log. Fsyncs after the write for durability.
    pub fn append(&mut self, event: &Event) -> Result<()> {
        let envelope = to_envelope(event, Utc::now());
        let mut line =
            serde_json::to_string(&envelope).context("failed to serialise event envelope")?;
        line.push('\n');
        self.file
            .write_all(line.as_bytes())
            .context("failed to write event to log")?;
        self.file.flush().context("failed to flush event log")?;
        #[cfg(not(target_os = "windows"))]
        self.file.sync_data().context("failed to fsync event log")?;
        #[cfg(target_os = "windows")]
        self.file.sync_data().context("failed to fsync event log")?;
        self.event_count += 1;
        Ok(())
    }

    /// Read all events from the file, skipping `skip` lines at the start.
    /// Malformed lines are warned and skipped; parsing does not stop.
    pub fn load_from(path: &Path, skip: usize) -> Result<Vec<Event>> {
        if !path.exists() {
            return Ok(vec![]);
        }
        let f = File::open(path)
            .with_context(|| format!("failed to open event log for reading: {}", path.display()))?;
        let reader = BufReader::new(f);
        let mut events = Vec::new();
        for (i, line) in reader.lines().enumerate() {
            if i < skip {
                continue;
            }
            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    warn!(line = i, "event log: I/O error reading line: {e}");
                    continue;
                }
            };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            match serde_json::from_str(trimmed).and_then(from_envelope) {
                Ok(ev) => events.push(ev),
                Err(e) => {
                    warn!(line = i, "event log: skipping malformed line: {e}");
                }
            }
        }
        Ok(events)
    }

    /// Atomically write a snapshot. Writes to `.tmp` first, then renames.
    pub fn snapshot_write(path: &Path, state: &State, event_count: usize) -> Result<()> {
        let tmp = path.with_extension("tmp");
        let snap = SnapshotFile {
            event_count,
            state: state.clone(),
        };
        let data = serde_json::to_string(&snap).context("failed to serialise snapshot")?;
        std::fs::write(&tmp, data)
            .with_context(|| format!("failed to write snapshot tmp: {}", tmp.display()))?;
        std::fs::rename(&tmp, path)
            .with_context(|| format!("failed to rename snapshot: {}", path.display()))?;
        Ok(())
    }

    /// Read a snapshot. Returns `None` if the file is absent.
    /// Returns `(state, events_consumed_count)` on success.
    pub fn snapshot_read(path: &Path) -> Result<Option<(State, usize)>> {
        if !path.exists() {
            return Ok(None);
        }
        let data = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read snapshot: {}", path.display()))?;
        let snap: SnapshotFile =
            serde_json::from_str(&data).with_context(|| "failed to parse snapshot")?;
        Ok(Some((snap.state, snap.event_count)))
    }
}

fn count_lines(path: &Path) -> Result<usize> {
    if !path.exists() {
        return Ok(0);
    }
    let f = File::open(path)?;
    let reader = BufReader::new(f);
    Ok(reader.lines().count())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use seed_core::{ReminderId, TraitId, initial_state};
    use tempfile::TempDir;

    const NOW_MS: i64 = 1_745_000_000_000;

    fn water_completed() -> Event {
        Event::ReminderCompleted {
            reminder_id: ReminderId("water".into()),
            xp_gained: 74,
            trait_id: TraitId("flow".into()),
            new_xp: 1234,
            streak: 5,
            at_ms: NOW_MS,
        }
    }

    #[test]
    fn append_then_load_round_trip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("events.jsonl");

        let mut log = EventLog::open(&path).unwrap();
        log.append(&water_completed()).unwrap();
        log.append(&Event::ReminderEnabled {
            reminder_id: ReminderId("eyes".into()),
        })
        .unwrap();
        assert_eq!(log.event_count, 2);

        let loaded = EventLog::load_from(&path, 0).unwrap();
        assert_eq!(loaded.len(), 2);
        assert!(matches!(loaded[0], Event::ReminderCompleted { .. }));
        assert!(matches!(loaded[1], Event::ReminderEnabled { .. }));
    }

    #[test]
    fn load_skips_correctly() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("events.jsonl");

        let mut log = EventLog::open(&path).unwrap();
        for _ in 0..5 {
            log.append(&water_completed()).unwrap();
        }

        let loaded = EventLog::load_from(&path, 3).unwrap();
        assert_eq!(loaded.len(), 2);
    }

    #[test]
    fn corrupted_line_skipped_gracefully() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("events.jsonl");

        // Write two valid events with a corrupt line in the middle.
        let mut log = EventLog::open(&path).unwrap();
        log.append(&water_completed()).unwrap();
        drop(log);

        // Inject garbage.
        let mut f = OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(b"{not valid json}\n").unwrap();
        drop(f);

        let mut log2 = EventLog::open(&path).unwrap();
        log2.append(&Event::ReminderEnabled {
            reminder_id: ReminderId("eyes".into()),
        })
        .unwrap();
        drop(log2);

        let loaded = EventLog::load_from(&path, 0).unwrap();
        // Corrupt line skipped; 2 valid events remain.
        assert_eq!(loaded.len(), 2);
    }

    #[test]
    fn snapshot_round_trip() {
        let dir = TempDir::new().unwrap();
        let snap_path = dir.path().join("snapshot.json");

        let state = initial_state(NOW_MS);
        EventLog::snapshot_write(&snap_path, &state, 42).unwrap();

        let (s2, count) = EventLog::snapshot_read(&snap_path).unwrap().unwrap();
        assert_eq!(count, 42);
        assert_eq!(s2.completed_total, state.completed_total);
        assert_eq!(s2.traits.len(), state.traits.len());
    }

    #[test]
    fn snapshot_read_absent_returns_none() {
        let dir = TempDir::new().unwrap();
        let snap_path = dir.path().join("snapshot.json");
        let result = EventLog::snapshot_read(&snap_path).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn open_existing_log_counts_lines_correctly() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("events.jsonl");

        // Write 3 events, close, reopen.
        {
            let mut log = EventLog::open(&path).unwrap();
            for _ in 0..3 {
                log.append(&water_completed()).unwrap();
            }
        }
        let log2 = EventLog::open(&path).unwrap();
        assert_eq!(log2.event_count, 3);
    }

    #[test]
    fn load_from_nonexistent_path_returns_empty() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nope.jsonl");
        let events = EventLog::load_from(&path, 0).unwrap();
        assert!(events.is_empty());
    }
}
