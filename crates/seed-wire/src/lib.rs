//! `seed-wire` — IPC wire protocol shared by `seedd`, `seed` (TUI), and any
//! other client that talks to the daemon socket (e.g. `seed-bridge`).
//!
//! Framing: each message is preceded by a 4-byte big-endian length (in bytes),
//! then a UTF-8 JSON body. Max frame size is 4 MiB.
//!
//! Protocol summary:
//! 1. Client connects to `@seed-daemon.sock` (abstract namespace).
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

/// Stable abstract-namespace socket name on Unix; per-user named pipe on Windows.
///
/// Returned as a `&'static str` so callers can build the platform-specific
/// `Name` via `interprocess`'s `ToNsName` traits without coupling `seed-wire`
/// to that dependency.
#[cfg(unix)]
pub const SOCKET_NAME: &str = "@seed-daemon.sock";

#[cfg(windows)]
pub fn socket_pipe_name() -> String {
    let username = std::env::var("USERNAME").unwrap_or_else(|_| "seedd".to_string());
    format!("seed-daemon-{username}")
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
}
