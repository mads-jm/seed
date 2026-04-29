/// Command parser: parses user input from the command bar.
use seed_core::domain::{CATEGORIES, FocusPattern, REMINDERS};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedCommand {
    /// A reminder verb (e.g. "water", "breathe").
    Verb { word: String },
    /// Debug command to set a trait to a specific level: `/flow 50`.
    Debug { trait_name: String, level: u8 },
    /// Debug command to randomize all 9 trait levels (1-99): `/random`.
    Random,
    /// Debug command to set all 9 traits to a fixed level (1-99): `/all N`.
    All { level: u8 },
    /// `help` or `?` — show help in log.
    Help,
    /// `?<skill>` — open the skill detail panel for the named trait.
    HelpSkill { skill: String },
    /// `?<unknown>` — user typed `?` with a skill name that doesn't exist.
    UnknownSkill(String),
    /// `/integrate <trait_id> [enhancement_id]`
    Integrate {
        trait_name: String,
        enhancement_name: Option<String>,
    },
    /// `/focus <pattern> <trait1> [trait2] [trait3]`
    Focus {
        pattern: FocusPattern,
        traits: Vec<String>,
    },
    /// Parser-level error for a `/focus` or `/integrate` command.
    PrestigeError(String),
    /// Unrecognized input.
    Unknown(String),
}

/// Parse a raw command string from the input bar.
///
/// Rules:
/// - Trims whitespace, lowercases.
/// - `help` or `?` → `Help`
/// - `/trait n` → `Debug { trait_name, level }`
/// - Known reminder word → `Verb { word }`
/// - Anything else → `Unknown`
pub fn parse(input: &str) -> ParsedCommand {
    let s = input.trim().to_lowercase();
    if s.is_empty() {
        return ParsedCommand::Unknown(String::new());
    }

    if s == "help" || s == "?" {
        return ParsedCommand::Help;
    }

    // `?<skill>` — open skill detail if the skill is a known trait id.
    if let Some(skill) = s.strip_prefix('?') {
        if !skill.is_empty() {
            let valid: bool = CATEGORIES.iter().any(|c| c.trait_id == skill);
            if valid {
                return ParsedCommand::HelpSkill {
                    skill: skill.to_string(),
                };
            }
        }
        // `?bogus` → UnknownSkill: tell the user their skill name was wrong.
        return ParsedCommand::UnknownSkill(skill.to_string());
    }

    if let Some(rest) = s.strip_prefix('/') {
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.len() == 1 && parts[0] == "random" {
            return ParsedCommand::Random;
        }

        // `/integrate <trait_id> [enhancement_id]`
        if !parts.is_empty() && parts[0] == "integrate" {
            if parts.len() < 2 {
                return ParsedCommand::PrestigeError(
                    "usage: /integrate <trait_id> [enhancement_id]".into(),
                );
            }
            let trait_name = parts[1].to_string();
            let valid_traits: Vec<&'static str> = CATEGORIES.iter().map(|c| c.trait_id).collect();
            if !valid_traits.contains(&trait_name.as_str()) {
                return ParsedCommand::PrestigeError(format!("unknown trait: {trait_name}"));
            }
            let enhancement_name = parts.get(2).map(|s| s.to_string());
            return ParsedCommand::Integrate {
                trait_name,
                enhancement_name,
            };
        }

        // `/focus <pattern> <trait1> [trait2] [trait3]`
        if !parts.is_empty() && parts[0] == "focus" {
            if parts.len() < 3 {
                return ParsedCommand::PrestigeError(
                    "usage: /focus <pattern> <trait1> [trait2] [trait3]".into(),
                );
            }
            let pattern = match parts[1] {
                "1x4" | "concentrate" | "concentrate1x4" => FocusPattern::Concentrate1x4,
                "2x3" | "spread2" | "spread2x3" => FocusPattern::Spread2x3,
                "3x2" | "spread3" | "spread3x2" => FocusPattern::Spread3x2,
                other => {
                    return ParsedCommand::PrestigeError(format!(
                        "unknown pattern '{other}' — use 1x4, 2x3, or 3x2"
                    ));
                }
            };
            let required = pattern.skill_count();
            let trait_args: Vec<String> = parts[2..].iter().map(|s| s.to_string()).collect();
            if trait_args.len() != required {
                return ParsedCommand::PrestigeError(format!(
                    "pattern requires {required} traits, got {}",
                    trait_args.len()
                ));
            }
            let valid_traits: Vec<&'static str> = CATEGORIES.iter().map(|c| c.trait_id).collect();
            for t in &trait_args {
                if !valid_traits.contains(&t.as_str()) {
                    return ParsedCommand::PrestigeError(format!("unknown trait: {t}"));
                }
            }
            return ParsedCommand::Focus {
                pattern,
                traits: trait_args,
            };
        }

        if parts.len() == 2 {
            // `/all N` — set every trait to level N (1-99 clamped).
            if parts[0] == "all" {
                if let Ok(n) = parts[1].parse::<u16>() {
                    let level = n.clamp(1, 99) as u8;
                    return ParsedCommand::All { level };
                }
                // `/all foo` — non-numeric arg → Unknown.
                return ParsedCommand::Unknown(s);
            }

            let trait_name = parts[0].to_string();
            let valid_traits: Vec<&'static str> = CATEGORIES.iter().map(|c| c.trait_id).collect();
            if valid_traits.contains(&trait_name.as_str())
                && let Ok(n) = parts[1].parse::<u16>()
            {
                let clamped = n.clamp(1, 99) as u8;
                return ParsedCommand::Debug {
                    trait_name,
                    level: clamped,
                };
            }
        }
        // Malformed debug command — explain and treat as Unknown.
        return ParsedCommand::Unknown(s);
    }

    // Match against reminder words.
    if REMINDERS.iter().any(|r| r.word == s) {
        return ParsedCommand::Verb { word: s };
    }

    ParsedCommand::Unknown(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_help() {
        assert_eq!(parse("help"), ParsedCommand::Help);
        assert_eq!(parse("HELP"), ParsedCommand::Help);
        assert_eq!(parse("?"), ParsedCommand::Help);
    }

    #[test]
    fn parse_help_skill_known_trait() {
        assert_eq!(
            parse("?flow"),
            ParsedCommand::HelpSkill {
                skill: "flow".into()
            }
        );
    }

    #[test]
    fn parse_help_skill_unknown_falls_back_to_help() {
        assert_eq!(parse("?bogus"), ParsedCommand::UnknownSkill("bogus".into()));
    }

    #[test]
    fn parse_help_skill_all_traits() {
        for cat in seed_core::domain::CATEGORIES {
            let cmd = format!("?{}", cat.trait_id);
            assert_eq!(
                parse(&cmd),
                ParsedCommand::HelpSkill {
                    skill: cat.trait_id.to_string()
                },
                "?{} should parse as HelpSkill",
                cat.trait_id
            );
        }
    }

    #[test]
    fn parse_verb_water() {
        assert_eq!(
            parse("water"),
            ParsedCommand::Verb {
                word: "water".into()
            }
        );
    }

    #[test]
    fn parse_verb_breathe() {
        assert_eq!(
            parse("breathe"),
            ParsedCommand::Verb {
                word: "breathe".into()
            }
        );
    }

    #[test]
    fn parse_debug_flow() {
        assert_eq!(
            parse("/flow 50"),
            ParsedCommand::Debug {
                trait_name: "flow".into(),
                level: 50
            }
        );
    }

    #[test]
    fn parse_debug_clamps_to_99() {
        match parse("/flow 200") {
            ParsedCommand::Debug { level, .. } => assert_eq!(level, 99),
            other => panic!("expected Debug, got {other:?}"),
        }
    }

    #[test]
    fn parse_debug_clamps_min_to_1() {
        match parse("/flow 0") {
            ParsedCommand::Debug { level, .. } => assert_eq!(level, 1),
            other => panic!("expected Debug, got {other:?}"),
        }
    }

    #[test]
    fn parse_unknown_word() {
        assert_eq!(parse("foobar"), ParsedCommand::Unknown("foobar".into()));
    }

    #[test]
    fn parse_random() {
        assert_eq!(parse("/random"), ParsedCommand::Random);
        assert_eq!(parse("/RANDOM"), ParsedCommand::Random);
        assert_eq!(parse("  /random  "), ParsedCommand::Random);
    }

    #[test]
    fn parse_random_with_extra_args_is_unknown() {
        // /random takes no args
        assert_eq!(
            parse("/random foo"),
            ParsedCommand::Unknown("/random foo".into())
        );
    }

    #[test]
    fn parse_unknown_bad_debug_trait() {
        // "blorp" is not a valid trait
        assert_eq!(
            parse("/blorp 50"),
            ParsedCommand::Unknown("/blorp 50".into())
        );
    }

    #[test]
    fn parse_empty_string() {
        assert_eq!(parse(""), ParsedCommand::Unknown(String::new()));
        assert_eq!(parse("   "), ParsedCommand::Unknown(String::new()));
    }

    #[test]
    fn parse_all_reminder_words_as_verbs() {
        use seed_core::domain::REMINDERS;
        for r in REMINDERS {
            let result = parse(r.word);
            assert_eq!(
                result,
                ParsedCommand::Verb {
                    word: r.word.to_string()
                },
                "reminder word '{}' should parse as Verb",
                r.word
            );
        }
    }

    #[test]
    fn parse_all_trait_debug_commands() {
        use seed_core::domain::CATEGORIES;
        for c in CATEGORIES {
            let cmd = format!("/{} 42", c.trait_id);
            match parse(&cmd) {
                ParsedCommand::Debug { trait_name, level } => {
                    assert_eq!(trait_name, c.trait_id);
                    assert_eq!(level, 42);
                }
                other => panic!("expected Debug for {cmd}, got {other:?}"),
            }
        }
    }

    #[test]
    fn parse_all_basic() {
        assert_eq!(parse("/all 50"), ParsedCommand::All { level: 50 });
    }

    #[test]
    fn parse_all_clamps_to_99() {
        assert_eq!(parse("/all 200"), ParsedCommand::All { level: 99 });
    }

    #[test]
    fn parse_all_clamps_min_to_1() {
        assert_eq!(parse("/all 0"), ParsedCommand::All { level: 1 });
    }

    #[test]
    fn parse_all_non_numeric_is_unknown() {
        assert_eq!(parse("/all foo"), ParsedCommand::Unknown("/all foo".into()));
    }

    // -----------------------------------------------------------------------
    // /integrate and /focus command tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_integrate_default_enhancement() {
        let result = parse("/integrate flow");
        assert_eq!(
            result,
            ParsedCommand::Integrate {
                trait_name: "flow".into(),
                enhancement_name: None,
            }
        );
    }

    #[test]
    fn parse_integrate_explicit_enhancement() {
        let result = parse("/integrate flow CoreEmber");
        // Parser passes the enhancement name through (daemon validates trait/enhancement match).
        assert_eq!(
            result,
            ParsedCommand::Integrate {
                trait_name: "flow".into(),
                enhancement_name: Some("coreember".into()),
            }
        );
    }

    #[test]
    fn parse_integrate_unknown_trait_returns_error() {
        let result = parse("/integrate bogus");
        assert!(
            matches!(result, ParsedCommand::PrestigeError(_)),
            "unknown trait should return PrestigeError"
        );
    }

    #[test]
    fn parse_integrate_no_args_returns_error() {
        let result = parse("/integrate");
        assert!(
            matches!(result, ParsedCommand::PrestigeError(_)),
            "no args should return PrestigeError"
        );
    }

    #[test]
    fn parse_focus_spread3x2() {
        let result = parse("/focus 3x2 flow core spine");
        assert_eq!(
            result,
            ParsedCommand::Focus {
                pattern: FocusPattern::Spread3x2,
                traits: vec!["flow".into(), "core".into(), "spine".into()],
            }
        );
    }

    #[test]
    fn parse_focus_concentrate() {
        let result = parse("/focus 1x4 flow");
        assert_eq!(
            result,
            ParsedCommand::Focus {
                pattern: FocusPattern::Concentrate1x4,
                traits: vec!["flow".into()],
            }
        );
    }

    #[test]
    fn parse_focus_arity_mismatch_returns_error() {
        // 2x3 requires 2 traits; providing 1 → PrestigeError.
        let result = parse("/focus 2x3 flow");
        assert!(
            matches!(result, ParsedCommand::PrestigeError(_)),
            "arity mismatch should return PrestigeError, got {result:?}"
        );
    }

    #[test]
    fn parse_focus_unknown_pattern_returns_error() {
        let result = parse("/focus 5x5 flow");
        assert!(
            matches!(result, ParsedCommand::PrestigeError(_)),
            "unknown pattern should return PrestigeError"
        );
    }

    #[test]
    fn parse_focus_unknown_trait_returns_error() {
        let result = parse("/focus 1x4 bogus");
        assert!(
            matches!(result, ParsedCommand::PrestigeError(_)),
            "unknown trait in /focus should return PrestigeError"
        );
    }

    #[test]
    fn parse_focus_alternative_pattern_names() {
        // "concentrate" → Concentrate1x4
        assert_eq!(
            parse("/focus concentrate flow"),
            ParsedCommand::Focus {
                pattern: FocusPattern::Concentrate1x4,
                traits: vec!["flow".into()],
            }
        );
        // "spread2" → Spread2x3
        assert_eq!(
            parse("/focus spread2 flow core"),
            ParsedCommand::Focus {
                pattern: FocusPattern::Spread2x3,
                traits: vec!["flow".into(), "core".into()],
            }
        );
    }
}
