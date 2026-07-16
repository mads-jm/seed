//! `seed-wire` — IPC wire protocol shared by `seedd`, `seed` (TUI), and any
//! other client that talks to the daemon socket (e.g. `seed-bridge`).
//!
//! Framing: each message is preceded by a 4-byte big-endian length (in bytes),
//! then a UTF-8 JSON body. Max frame size is 4 MiB.
//!
//! Protocol summary:
//! 1. Client connects to the socket serving its `SEED_HOME` (abstract
//!    namespace; `@seed-daemon.sock` for the default home). See
//!    [`socket_name_for`] — one socket per seed home, so sandboxed runs are
//!    isolated from the real daemon.
//! 2. Daemon sends `Hello`.
//! 3. Client sends `Request { id, action: Subscribe }`.
//! 4. Daemon replies with `Snapshot { state }` on the same connection.
//! 5. Daemon broadcasts `StateDiff { events }` to every subscriber on every commit.
//!
//! `Action` and `Event` variants are additive — new variants must round-trip
//! through `Event::Unknown` for forward compat.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use seed_core::{
    EventEnvelope, ReminderId, State, TraitId,
    domain::{FocusPattern, IntegrationEnhancement},
};

pub const MAX_FRAME: usize = 4 * 1024 * 1024; // 4 MiB

/// Current protocol version advertised in `Hello.protocol_version`.
/// Bumped only on a breaking schema change. Adding a variant is non-breaking.
pub const PROTOCOL_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Top-level message
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Message {
    Request {
        id: u64,
        action: Action,
    },
    Response {
        id: u64,
        result: ResponseResult,
    },
    /// Pushed to all subscribers whenever events commit.
    StateDiff {
        events: Vec<EventEnvelope>,
    },
    /// Full state snapshot sent once on Subscribe.
    Snapshot {
        state: Box<State>,
    },
    Hello {
        daemon_version: String,
        protocol_version: u32,
    },
    Ping,
    Pong,
    Error {
        message: String,
    },
}

/// Wrapper so we can serde a `Result<Value, String>` as a tagged union.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ResponseResult {
    Ok { value: Value },
    Err { message: String },
}

impl ResponseResult {
    pub fn ok(v: impl Serialize) -> Self {
        ResponseResult::Ok {
            value: serde_json::to_value(v).unwrap_or(Value::Null),
        }
    }
    pub fn err(msg: impl Into<String>) -> Self {
        ResponseResult::Err {
            message: msg.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Action enum
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Action {
    Complete {
        reminder_id: ReminderId,
    },
    Snooze {
        reminder_id: ReminderId,
        minutes: u32,
    },
    TogglePin {
        reminder_id: ReminderId,
    },
    ToggleEnabled {
        reminder_id: ReminderId,
    },
    SetPalette {
        palette: String,
    },
    SetTraitLevel {
        trait_id: TraitId,
        level: u8,
    },
    TriggerReminderNow {
        reminder_id: Option<ReminderId>,
    },
    Subscribe,
    Shutdown,
    /// Reset all companion progress to initial_state. Emits CompanionAwakened.
    Reset,
    SetReminderInterval {
        reminder_id: ReminderId,
        minutes: u32,
    },
    SetXpMultiplier {
        multiplier: u32,
    },
    /// Integrate a trait at level 99: reset XP to 0 and apply a visual enhancement.
    Integrate {
        trait_id: TraitId,
        enhancement_id: IntegrationEnhancement,
    },
    /// Spend one focus token to activate a focus phase with XP multipliers.
    ActivateFocusPhase {
        pattern: FocusPattern,
        traits: Vec<TraitId>,
    },
}

// ---------------------------------------------------------------------------
// Socket name
// ---------------------------------------------------------------------------

/// Abstract-namespace socket name for the *default* seed home (`~/.seed`).
///
/// Non-default homes get their own name via [`socket_name_for`]; see that
/// function for why the default keeps this bare, un-suffixed name.
///
/// Names are produced as owned `String`s (this one as a `&'static str`) so
/// callers can build the platform-specific `Name` via `interprocess`'s
/// `ToNsName` traits without coupling `seed-wire` to that dependency.
#[cfg(unix)]
pub const SOCKET_NAME: &str = "@seed-daemon.sock";

/// FNV-1a (64-bit). Deliberately hand-rolled rather than `std`'s
/// `DefaultHasher`: std makes no stability guarantee across Rust releases, and
/// this hash names a socket that *separately-built binaries must agree on*. A
/// `seedd` and a `seed` compiled with different toolchains would otherwise
/// derive different names and never find each other. FNV-1a is fixed forever.
fn fnv1a64(bytes: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    let mut hash = OFFSET_BASIS;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// Resolve a seed home to a canonical form for identity purposes.
///
/// Falls back to the path as given when it doesn't exist yet (the daemon may
/// name its home before creating it). Both sides of a connection run this same
/// function over the same `SEED_HOME` value, so they agree either way.
fn normalized_home(seed_home: &Path) -> PathBuf {
    seed_home
        .canonicalize()
        .unwrap_or_else(|_| seed_home.to_path_buf())
}

/// The socket name serving a given seed home.
///
/// Each seed home gets its own socket, so a sandboxed run
/// (`SEED_HOME=/tmp/sandbox`) can't reach — or be reached by — the daemon
/// owning the real `~/.seed`. Before this, the name was a global constant and
/// `SEED_HOME` isolated state but *not* IPC: a sandboxed client would silently
/// connect to the live daemon and mutate real data.
///
/// The default home keeps the bare [`SOCKET_NAME`] rather than a hashed one.
/// That's a compatibility decision, not an accident: an already-running daemon
/// listens on the old name, and if an upgraded client derived a *new* name for
/// the same home it would find nothing, spawn a second daemon, and put two
/// writers on one `events.jsonl`. Keeping the default stable makes the upgrade
/// a no-op for every existing install.
///
/// Pure and deterministic given the path — see the unit tests. Use
/// [`socket_name`] for the env-resolved name.
pub fn socket_name_for(seed_home: &Path) -> String {
    let home = normalized_home(seed_home);
    let is_default = home == normalized_home(&seed_core::default_seed_home());

    #[cfg(unix)]
    {
        if is_default {
            return SOCKET_NAME.to_string();
        }
        let hash = fnv1a64(home.to_string_lossy().as_bytes());
        format!("@seed-daemon-{hash:016x}.sock")
    }
    #[cfg(windows)]
    {
        let username = std::env::var("USERNAME").unwrap_or_else(|_| "seedd".to_string());
        if is_default {
            return format!("seed-daemon-{username}");
        }
        let hash = fnv1a64(home.to_string_lossy().as_bytes());
        format!("seed-daemon-{username}-{hash:016x}")
    }
}

/// The socket name for the seed home this process is configured to use.
///
/// Resolves `SEED_HOME` via [`seed_core::seed_home`], so every binary in the
/// workspace (`seedd`, `seed`, `seed-bridge`) derives the same name from the
/// same environment. `spawn_daemon` passes `SEED_HOME` through explicitly, so a
/// daemon spawned by a client always agrees with it.
pub fn socket_name() -> String {
    socket_name_for(&seed_core::seed_home())
}

// ---------------------------------------------------------------------------
// Framing
// ---------------------------------------------------------------------------

/// Write a length-prefixed JSON frame: 4-byte big-endian length + UTF-8 body.
pub async fn write_frame(writer: &mut (impl AsyncWrite + Unpin), msg: &Message) -> Result<()> {
    let body = serde_json::to_vec(msg)?;
    if body.len() > MAX_FRAME {
        bail!("outgoing frame too large: {} bytes", body.len());
    }
    let len = (body.len() as u32).to_be_bytes();
    writer.write_all(&len).await?;
    writer.write_all(&body).await?;
    writer.flush().await?;
    Ok(())
}

/// Read a length-prefixed JSON frame.
/// Returns `None` on clean EOF (zero bytes read for the length prefix).
/// Returns `Err` on truncated data, oversized frame, or malformed JSON.
pub async fn read_frame(reader: &mut (impl AsyncRead + Unpin)) -> Result<Option<Message>> {
    let mut len_buf = [0u8; 4];
    match reader.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME {
        bail!(
            "incoming frame too large: {} bytes (max {})",
            len,
            MAX_FRAME
        );
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).await?;
    let msg = serde_json::from_slice(&body)?;
    Ok(Some(msg))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    async fn round_trip(msg: &Message) -> Message {
        let mut buf = Vec::new();
        write_frame(&mut buf, msg).await.unwrap();
        let mut reader = BufReader::new(buf.as_slice());
        read_frame(&mut reader).await.unwrap().unwrap()
    }

    #[tokio::test]
    async fn ping_pong_round_trip() {
        let msg = Message::Ping;
        let rt = round_trip(&msg).await;
        assert!(matches!(rt, Message::Ping));
    }

    #[tokio::test]
    async fn hello_round_trip() {
        let msg = Message::Hello {
            daemon_version: "0.1.0".into(),
            protocol_version: 1,
        };
        let rt = round_trip(&msg).await;
        match rt {
            Message::Hello {
                daemon_version,
                protocol_version,
            } => {
                assert_eq!(daemon_version, "0.1.0");
                assert_eq!(protocol_version, 1);
            }
            other => panic!("expected Hello, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn request_complete_round_trip() {
        let msg = Message::Request {
            id: 42,
            action: Action::Complete {
                reminder_id: ReminderId("water".into()),
            },
        };
        let rt = round_trip(&msg).await;
        match rt {
            Message::Request {
                id,
                action: Action::Complete { reminder_id },
            } => {
                assert_eq!(id, 42);
                assert_eq!(reminder_id.0, "water");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[tokio::test]
    async fn clean_eof_returns_none() {
        let empty: &[u8] = &[];
        let mut reader = tokio::io::BufReader::new(empty);
        let result = read_frame(&mut reader).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn oversized_frame_rejected() {
        // Write a length header claiming 5 MiB.
        let len: u32 = (5 * 1024 * 1024) as u32;
        let buf = len.to_be_bytes().to_vec();
        let mut reader = tokio::io::BufReader::new(buf.as_slice());
        let result = read_frame(&mut reader).await;
        assert!(result.is_err(), "expected error on oversized frame");
    }

    #[tokio::test]
    async fn truncated_body_returns_err() {
        // Write length=100 but only 10 bytes of body.
        let mut buf = 100u32.to_be_bytes().to_vec();
        buf.extend_from_slice(&[0u8; 10]);
        let mut reader = tokio::io::BufReader::new(buf.as_slice());
        let result = read_frame(&mut reader).await;
        assert!(result.is_err(), "expected error on truncated frame body");
    }

    #[cfg(unix)]
    #[test]
    fn socket_name_is_abstract() {
        assert_eq!(SOCKET_NAME, "@seed-daemon.sock");
    }

    // -----------------------------------------------------------------------
    // Per-home socket naming
    //
    // These call `socket_name_for` directly (never the env-reading
    // `socket_name`), so they stay deterministic and race-free when the test
    // binary runs them in parallel.
    // -----------------------------------------------------------------------

    #[cfg(unix)]
    #[test]
    fn default_home_keeps_the_legacy_socket_name() {
        // The compat guarantee: an upgraded client must still find the daemon
        // already listening on the bare name, or it would spawn a second writer
        // against the same ~/.seed.
        assert_eq!(
            socket_name_for(&seed_core::default_seed_home()),
            SOCKET_NAME
        );
    }

    #[cfg(unix)]
    #[test]
    fn non_default_home_gets_its_own_socket() {
        let name = socket_name_for(Path::new("/tmp/seed-sandbox-xyz"));
        assert_ne!(
            name, SOCKET_NAME,
            "a sandboxed home must not share the real daemon's socket"
        );
        assert!(
            name.starts_with("@seed-daemon-"),
            "abstract namespace: {name}"
        );
        assert!(name.ends_with(".sock"), "{name}");
    }

    #[test]
    fn distinct_homes_get_distinct_sockets() {
        assert_ne!(
            socket_name_for(Path::new("/tmp/seed-a")),
            socket_name_for(Path::new("/tmp/seed-b")),
        );
    }

    #[test]
    fn same_home_is_stable_across_calls() {
        // Daemon and client derive the name independently; they must agree.
        assert_eq!(
            socket_name_for(Path::new("/tmp/seed-stable")),
            socket_name_for(Path::new("/tmp/seed-stable")),
        );
    }

    #[test]
    fn fnv1a_matches_known_vectors() {
        // Pinned so the hash can never drift: separately-built binaries must
        // derive identical names. Canonical FNV-1a 64-bit test vectors.
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
    }
}
