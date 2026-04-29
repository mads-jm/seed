/// Config loader. Reads `<seed_home>/config.toml` if present, merges over
/// hard-coded defaults. Missing keys fall through to defaults — never panics.
///
/// Defaults are aligned to `state.jsx::initialState`:
///   - `snooze_min = 5`  (JSX: `snoozeMin: 5`)
///   - `notif_style = flash`  (JSX: `notifStyle: 'flash'`)
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::domain::{REMINDERS, ReminderId};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Known palette identifiers.
const VALID_PALETTES: &[&str] = &["sage", "dusk", "mist", "ember", "moss"];

/// JSX baseline: `notifStyle: 'flash'`
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotifStyle {
    Standard,
    #[default]
    Flash,
    Silent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReminderOverride {
    /// Override interval in minutes. `None` = use static default.
    pub interval_min: Option<u32>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Active notification window: (start_hour, end_hour), 24-hour clock.
    /// Both values must be in 0..=23 with start ≤ end.
    pub active_hours: (u8, u8),
    /// JSX baseline: `snoozeMin: 5`
    pub snooze_min: u32,
    /// Palette name: sage | dusk | mist | ember | moss.
    pub palette: String,
    pub reminders: BTreeMap<ReminderId, ReminderOverride>,
    /// JSX baseline: `notifStyle: 'flash'`
    pub notif_style: NotifStyle,
    pub glyph_seed: u64,
}

impl Default for Config {
    fn default() -> Self {
        let reminders = REMINDERS
            .iter()
            .map(|r| {
                (
                    r.reminder_id(),
                    ReminderOverride {
                        interval_min: None,
                        enabled: Some(true),
                    },
                )
            })
            .collect();

        Config {
            active_hours: (7, 22),
            snooze_min: 5,
            palette: "sage".to_string(),
            reminders,
            notif_style: NotifStyle::Flash,
            glyph_seed: 42,
        }
    }
}

// ---------------------------------------------------------------------------
// TOML partial overlay types
// ---------------------------------------------------------------------------

/// Partial TOML representation — every field is optional so missing keys fall
/// through to defaults.
#[derive(Debug, Default, Deserialize)]
struct PartialConfig {
    active_hours: Option<(u8, u8)>,
    snooze_min: Option<u32>,
    palette: Option<String>,
    reminders: Option<BTreeMap<String, PartialReminderOverride>>,
    notif_style: Option<NotifStyle>,
    glyph_seed: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
struct PartialReminderOverride {
    interval_min: Option<u32>,
    enabled: Option<bool>,
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Returns the set of known reminder ID strings (from the static catalog).
fn known_reminder_ids() -> Vec<&'static str> {
    REMINDERS.iter().map(|r| r.id).collect()
}

fn validate_active_hours(hours: (u8, u8)) -> Result<()> {
    let (start, end) = hours;
    if start > 23 {
        bail!("active_hours start value {start} is out of range; must be 0..=23");
    }
    if end > 23 {
        bail!("active_hours end value {end} is out of range; must be 0..=23");
    }
    if start >= end {
        bail!(
            "active_hours start ({start}) must be < end ({end}); \
             a zero-length window (start == end) silently disables all notifications. \
             Wrap-around windows (e.g. 22..7) are not supported"
        );
    }
    Ok(())
}

fn validate_palette(palette: &str) -> Result<()> {
    if !VALID_PALETTES.contains(&palette) {
        bail!(
            "unknown palette {:?}; valid values are: {}",
            palette,
            VALID_PALETTES.join(", ")
        );
    }
    Ok(())
}

fn validate_reminder_ids(overrides: &BTreeMap<String, PartialReminderOverride>) -> Result<()> {
    let known = known_reminder_ids();
    let unknowns: Vec<&str> = overrides
        .keys()
        .filter(|id| !known.contains(&id.as_str()))
        .map(|s| s.as_str())
        .collect();
    if !unknowns.is_empty() {
        bail!(
            "config contains unknown reminder id(s): {}; \
             valid ids are: {}",
            unknowns.join(", "),
            known.join(", ")
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Load / scaffold
// ---------------------------------------------------------------------------

/// Load config from `<seed_home>/config.toml`, merging over defaults.
/// If the file is absent or empty, returns `Config::default()`.
/// Returns `Err` for malformed TOML, invalid palette, invalid active_hours,
/// or unknown reminder IDs.
pub fn load(seed_home: &Path) -> Result<Config> {
    let path = seed_home.join("config.toml");
    if !path.exists() {
        return Ok(Config::default());
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read config file: {}", path.display()))?;
    if raw.trim().is_empty() {
        return Ok(Config::default());
    }
    let partial: PartialConfig = toml::from_str(&raw)
        .with_context(|| format!("failed to parse config file: {}", path.display()))?;

    // Validate fields before merging.
    if let Some(hours) = partial.active_hours {
        validate_active_hours(hours)
            .with_context(|| format!("invalid active_hours in {}", path.display()))?;
    }
    if let Some(ref palette) = partial.palette {
        validate_palette(palette)
            .with_context(|| format!("invalid palette in {}", path.display()))?;
    }
    if let Some(ref overrides) = partial.reminders {
        validate_reminder_ids(overrides)
            .with_context(|| format!("invalid reminder ids in {}", path.display()))?;
    }

    Ok(merge(Config::default(), partial))
}

fn merge(mut base: Config, partial: PartialConfig) -> Config {
    if let Some(v) = partial.active_hours {
        base.active_hours = v;
    }
    if let Some(v) = partial.snooze_min {
        base.snooze_min = v;
    }
    if let Some(v) = partial.palette {
        base.palette = v;
    }
    if let Some(v) = partial.notif_style {
        base.notif_style = v;
    }
    if let Some(v) = partial.glyph_seed {
        base.glyph_seed = v;
    }
    if let Some(overrides) = partial.reminders {
        for (id_str, pr) in overrides {
            let rid = ReminderId(id_str);
            let entry = base.reminders.entry(rid).or_insert(ReminderOverride {
                interval_min: None,
                enabled: Some(true),
            });
            if pr.interval_min.is_some() {
                entry.interval_min = pr.interval_min;
            }
            if pr.enabled.is_some() {
                entry.enabled = pr.enabled;
            }
        }
    }
    base
}

/// Write an annotated default `config.toml` to `<seed_home>/config.toml`.
/// Used by `seed init`. Does not overwrite an existing file.
pub fn scaffold_default(seed_home: &Path) -> Result<()> {
    let path = seed_home.join("config.toml");
    if path.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(seed_home)?;
    std::fs::write(path, annotated_default_toml())?;
    Ok(())
}

fn annotated_default_toml() -> String {
    let mut out = String::new();
    out.push_str("# seed configuration\n");
    out.push_str("# All keys are optional — omit any line to keep the default.\n\n");
    out.push_str("# Hours during which notifications are active (24-hour clock).\n");
    out.push_str("active_hours = [7, 22]\n\n");
    out.push_str("# Default snooze duration in minutes.\n");
    out.push_str("snooze_min = 5\n\n");
    out.push_str("# Colour palette: sage | dusk | mist | ember | moss\n");
    out.push_str("palette = \"sage\"\n\n");
    out.push_str("# Notification style: standard | flash | silent\n");
    out.push_str("notif_style = \"flash\"\n\n");
    out.push_str("# Deterministic seed for glyph generation.\n");
    out.push_str("glyph_seed = 42\n\n");
    out.push_str("# Per-reminder overrides. Omit any section to use the default.\n");
    out.push_str("# [reminders.water]\n");
    out.push_str("# interval_min = 45\n");
    out.push_str("# enabled = true\n");
    out
}

/// Resolve the seed home directory path (no I/O).
pub fn seed_home_path() -> PathBuf {
    std::env::var("SEED_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".seed")
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn default_config_active_hours() {
        let c = Config::default();
        assert_eq!(c.active_hours, (7, 22));
    }

    #[test]
    fn default_config_snooze_min_matches_jsx_baseline() {
        // JSX initialState: snoozeMin: 5
        let c = Config::default();
        assert_eq!(c.snooze_min, 5);
    }

    #[test]
    fn default_config_notif_style_matches_jsx_baseline() {
        // JSX initialState: notifStyle: 'flash'
        let c = Config::default();
        assert_eq!(c.notif_style, NotifStyle::Flash);
    }

    #[test]
    fn default_config_palette_is_sage() {
        let c = Config::default();
        assert_eq!(c.palette, "sage");
    }

    #[test]
    fn default_config_all_reminders_enabled() {
        let c = Config::default();
        assert_eq!(c.reminders.len(), 20);
        for (id, r) in &c.reminders {
            assert_eq!(
                r.enabled,
                Some(true),
                "reminder {id:?} should default to enabled"
            );
        }
    }

    #[test]
    fn load_absent_file_returns_default() {
        let dir = TempDir::new().unwrap();
        let c = load(dir.path()).unwrap();
        let d = Config::default();
        assert_eq!(c.snooze_min, d.snooze_min);
        assert_eq!(c.palette, d.palette);
    }

    #[test]
    fn partial_override_merges_correctly() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            "snooze_min = 15\npalette = \"dusk\"\n",
        )
        .unwrap();
        let c = load(dir.path()).unwrap();
        assert_eq!(c.snooze_min, 15);
        assert_eq!(c.palette, "dusk");
        // Defaults preserved
        assert_eq!(c.active_hours, (7, 22));
        assert_eq!(c.glyph_seed, 42);
    }

    #[test]
    fn reminder_override_partial_merge() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            "[reminders.water]\ninterval_min = 30\n",
        )
        .unwrap();
        let c = load(dir.path()).unwrap();
        let water = c.reminders.get(&ReminderId("water".into())).unwrap();
        assert_eq!(water.interval_min, Some(30));
        assert_eq!(water.enabled, Some(true));
        let eyes = c.reminders.get(&ReminderId("eyes".into())).unwrap();
        assert_eq!(eyes.interval_min, None);
    }

    #[test]
    fn malformed_toml_returns_err() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("config.toml"), "not = [valid toml{{{").unwrap();
        assert!(load(dir.path()).is_err());
    }

    #[test]
    fn scaffold_creates_file() {
        let dir = TempDir::new().unwrap();
        scaffold_default(dir.path()).unwrap();
        assert!(dir.path().join("config.toml").exists());
        let content = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(content.contains("active_hours"));
    }

    #[test]
    fn scaffold_does_not_overwrite_existing() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "palette = \"ember\"").unwrap();
        scaffold_default(dir.path()).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content.trim(), "palette = \"ember\"");
    }

    #[test]
    fn round_trip_default_config_toml() {
        let c = Config::default();
        let s = toml::to_string(&c).unwrap();
        let c2: Config = toml::from_str(&s).unwrap();
        assert_eq!(c2.snooze_min, c.snooze_min);
        assert_eq!(c2.palette, c.palette);
        assert_eq!(c2.active_hours, c.active_hours);
        assert_eq!(c2.glyph_seed, c.glyph_seed);
    }

    // -----------------------------------------------------------------------
    // Validation error paths (Fix 5)
    // -----------------------------------------------------------------------

    #[test]
    fn active_hours_start_out_of_range_returns_err() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("config.toml"), "active_hours = [25, 22]\n").unwrap();
        let result = load(dir.path());
        assert!(result.is_err(), "start=25 should be rejected");
        let msg = format!("{:?}", result.unwrap_err());
        assert!(
            msg.contains("out of range") || msg.contains("25"),
            "error: {msg}"
        );
    }

    #[test]
    fn active_hours_end_out_of_range_returns_err() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("config.toml"), "active_hours = [7, 99]\n").unwrap();
        let result = load(dir.path());
        assert!(result.is_err(), "end=99 should be rejected");
    }

    #[test]
    fn active_hours_start_greater_than_end_returns_err() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("config.toml"), "active_hours = [22, 7]\n").unwrap();
        let result = load(dir.path());
        assert!(result.is_err(), "start > end should be rejected");
    }

    #[test]
    fn active_hours_start_equal_end_returns_err() {
        // (7, 7) is a zero-length window — silently disables all notifications.
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("config.toml"), "active_hours = [7, 7]\n").unwrap();
        let result = load(dir.path());
        assert!(
            result.is_err(),
            "start == end should be rejected as zero-length window"
        );
        let msg = format!("{:?}", result.unwrap_err());
        assert!(
            msg.contains("zero-length") || msg.contains("7"),
            "error should mention the zero-length window: {msg}"
        );
    }

    #[test]
    fn unknown_palette_returns_err() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("config.toml"), "palette = \"neon\"\n").unwrap();
        let result = load(dir.path());
        assert!(result.is_err(), "unknown palette should be rejected");
        let msg = format!("{:?}", result.unwrap_err());
        assert!(
            msg.contains("neon") || msg.contains("palette"),
            "error: {msg}"
        );
    }

    #[test]
    fn all_valid_palettes_are_accepted() {
        let dir = TempDir::new().unwrap();
        for palette in &["sage", "dusk", "mist", "ember", "moss"] {
            std::fs::write(
                dir.path().join("config.toml"),
                format!("palette = \"{palette}\"\n"),
            )
            .unwrap();
            assert!(
                load(dir.path()).is_ok(),
                "palette {palette:?} should be accepted"
            );
        }
    }

    #[test]
    fn unknown_reminder_id_returns_err() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            "[reminders.totally_made_up]\nenabled = false\n",
        )
        .unwrap();
        let result = load(dir.path());
        assert!(result.is_err(), "unknown reminder id should be rejected");
        let msg = format!("{:?}", result.unwrap_err());
        assert!(
            msg.contains("totally_made_up"),
            "error should name the bad id: {msg}"
        );
    }

    #[test]
    fn known_reminder_id_typo_returns_err() {
        let dir = TempDir::new().unwrap();
        // "watter" is a typo for "water"
        std::fs::write(
            dir.path().join("config.toml"),
            "[reminders.watter]\nenabled = true\n",
        )
        .unwrap();
        assert!(load(dir.path()).is_err());
    }

    #[test]
    fn valid_reminder_id_is_accepted() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            "[reminders.water]\nenabled = false\n",
        )
        .unwrap();
        let c = load(dir.path()).unwrap();
        let water = c.reminders.get(&ReminderId("water".into())).unwrap();
        assert_eq!(water.enabled, Some(false));
    }
}
