use seed_core::{
    config::{Config, NotifStyle, load, scaffold_default},
    domain::ReminderId,
};
use tempfile::TempDir;

#[test]
fn absent_file_returns_defaults() {
    let dir = TempDir::new().unwrap();
    let c = load(dir.path()).unwrap();
    let d = Config::default();
    assert_eq!(c.snooze_min, d.snooze_min);
    assert_eq!(c.palette, d.palette);
    assert_eq!(c.active_hours, d.active_hours);
}

#[test]
fn partial_override_only_overrides_specified_keys() {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("config.toml"),
        "snooze_min = 20\npalette = \"ember\"\n",
    )
    .unwrap();
    let c = load(dir.path()).unwrap();

    assert_eq!(c.snooze_min, 20);
    assert_eq!(c.palette, "ember");
    // Defaults preserved
    assert_eq!(c.active_hours, (7, 22));
    assert_eq!(c.glyph_seed, 42);
    // notif_style falls through to default (flash — JSX baseline)
    assert_eq!(c.notif_style, NotifStyle::Flash);
}

#[test]
fn reminder_interval_override_preserves_enabled_default() {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("config.toml"),
        "[reminders.water]\ninterval_min = 60\n",
    )
    .unwrap();
    let c = load(dir.path()).unwrap();
    let water = c.reminders.get(&ReminderId("water".into())).unwrap();
    assert_eq!(water.interval_min, Some(60));
    assert_eq!(water.enabled, Some(true));
}

#[test]
fn all_twenty_reminders_in_default_config() {
    let c = Config::default();
    assert_eq!(c.reminders.len(), 20);
}

#[test]
fn malformed_toml_returns_err() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("config.toml"), "not valid toml }{{{").unwrap();
    // malformed TOML must now return Err (not silently use defaults + write to stderr)
    assert!(load(dir.path()).is_err());
}

#[test]
fn round_trip_default_config_through_toml() {
    let c = Config::default();
    let s = toml::to_string(&c).unwrap();
    let c2: Config = toml::from_str(&s).unwrap();
    assert_eq!(c2.snooze_min, c.snooze_min);
    assert_eq!(c2.palette, c.palette);
    assert_eq!(c2.active_hours, c.active_hours);
    assert_eq!(c2.glyph_seed, c.glyph_seed);
    assert_eq!(c2.reminders.len(), c.reminders.len());
}

#[test]
fn scaffold_creates_config_toml() {
    let dir = TempDir::new().unwrap();
    scaffold_default(dir.path()).unwrap();
    assert!(dir.path().join("config.toml").exists());
}

#[test]
fn scaffold_does_not_overwrite_existing() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "palette = \"dusk\"").unwrap();
    scaffold_default(dir.path()).unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    assert_eq!(content.trim(), "palette = \"dusk\"");
}

#[test]
fn notif_style_flash_is_default() {
    // JSX baseline: notifStyle: 'flash'
    let c = Config::default();
    assert_eq!(c.notif_style, NotifStyle::Flash);
}

#[test]
fn notif_style_standard_round_trips() {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("config.toml"),
        "notif_style = \"standard\"\n",
    )
    .unwrap();
    let c = load(dir.path()).unwrap();
    assert_eq!(c.notif_style, NotifStyle::Standard);
}

#[test]
fn active_hours_override() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("config.toml"), "active_hours = [8, 20]\n").unwrap();
    let c = load(dir.path()).unwrap();
    assert_eq!(c.active_hours, (8, 20));
}
