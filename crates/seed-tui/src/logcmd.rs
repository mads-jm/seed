/// Headless `seed log <verb>` subcommand.
///
/// Connects to a running `seedd` (auto-spawning one if none is reachable),
/// sends a single `Action::Complete` for the reminder resolved from `<verb>`,
/// prints the daemon-reported XP/level diff, and exits — without launching the
/// TUI. `seed log <verb> --json` emits the structured diff for status bars.
///
/// The daemon remains the sole writer: this path only sends the action and
/// reads the diff the daemon reports back (single request/response round-trip).
use anyhow::Result;
use seed_core::domain::REMINDERS;
use std::path::Path;

use crate::client::{Action, ResponseResult, ensure_daemon_ready, request_once};

/// Resolve a reminder verb word (e.g. "water") to its `ReminderId`.
///
/// Mirrors the TUI command parser's resolution (`REMINDERS.iter().find(...)`).
/// Returns `None` for an unknown verb.
fn resolve_verb(verb: &str) -> Option<seed_core::ReminderId> {
    let verb = verb.trim().to_lowercase();
    REMINDERS
        .iter()
        .find(|r| r.word == verb)
        .map(|r| r.reminder_id())
}

/// Run `seed log <verb> [--json]`.
///
/// `args` is the full process argv. Returns the intended process exit code:
/// 0 on a successful completion, non-zero on unknown verb, spawn/ready failure,
/// or a daemon-reported error.
pub async fn run(seed_home: &Path, args: &[String]) -> Result<i32> {
    let json = args.iter().any(|a| a == "--json");

    // The verb is the first positional after `log` that isn't a flag.
    // argv layout: [prog, "log", <verb>, ...flags] (flags may precede verb too).
    let verb = args.iter().skip(2).find(|a| !a.starts_with("--")).cloned();

    let Some(verb) = verb else {
        eprintln!("seed log: missing verb (e.g. `seed log water`)");
        return Ok(2);
    };

    let Some(reminder_id) = resolve_verb(&verb) else {
        eprintln!("seed log: unknown verb '{verb}'");
        return Ok(2);
    };

    // Ensure a daemon is reachable (spawns + waits if none), then one-shot.
    ensure_daemon_ready(seed_home).await?;

    let result = request_once(Action::Complete { reminder_id }).await?;

    match result {
        ResponseResult::Ok { value } => {
            print_diff(&value, json);
            Ok(0)
        }
        ResponseResult::Err { message } => {
            eprintln!("seed log: daemon error: {message}");
            Ok(1)
        }
    }
}

/// Print the completion diff. `--json` emits the raw daemon object; otherwise a
/// human single line, plus a distinct level-up line when the level advanced.
fn print_diff(value: &serde_json::Value, json: bool) {
    if json {
        println!("{value}");
        return;
    }

    let trait_name = value.get("trait").and_then(|v| v.as_str()).unwrap_or("?");
    let old_level = value.get("old_level").and_then(|v| v.as_u64()).unwrap_or(0);
    let new_level = value.get("new_level").and_then(|v| v.as_u64()).unwrap_or(0);
    let xp_delta = value.get("xp_delta").and_then(|v| v.as_u64()).unwrap_or(0);

    println!("+{xp_delta} XP · {trait_name} {old_level}→{new_level}");
    if new_level != old_level {
        println!("level up! {trait_name} reached level {new_level}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_known_verb() {
        // "water" is a canonical reminder word; resolves to a ReminderId.
        assert!(resolve_verb("water").is_some());
    }

    #[test]
    fn resolve_verb_case_insensitive_and_trimmed() {
        assert_eq!(resolve_verb("  WATER "), resolve_verb("water"));
        assert!(resolve_verb("  WATER ").is_some());
    }

    #[test]
    fn resolve_unknown_verb_is_none() {
        assert!(resolve_verb("bogus").is_none());
    }

    #[test]
    fn resolve_all_reminder_words() {
        for r in REMINDERS {
            assert_eq!(
                resolve_verb(r.word),
                Some(r.reminder_id()),
                "verb '{}' should resolve to its reminder id",
                r.word
            );
        }
    }
}
