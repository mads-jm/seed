# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Vault Is

This is an Obsidian documentation vault following the obsidian-docs-template canonical structure. It uses a Zettelkasten-inspired numbered directory layout and publishes to GitHub Pages via the Webpage HTML Export plugin.

## Canonical Directory Structure

Directories use numbered prefixes (`00`-`99`). The canonical set is defined in the template's `lib/common.sh`. Do not create new numbered directories without updating the template definitions.

| # | Directory | Purpose |
|---|-----------|---------|
| 00 | index/ | Navigation hub — `.base` views and kanban boards |
| 01 | concepts/ | Atomic, durable knowledge notes |
| 02 | references/ | API refs and external documentation |
| 03 | guides/ | Developer workflow guides |
| 04 | architecture/ | Design docs and ADRs (in `adr/` subfolder) |
| 05 | notes/ | Fleeting notes and working drafts |
| 06 | reports/ | Sprint reports and progress snapshots |
| 07 | stories/ | Vision documents and narratives |
| 08 | specs/ | Feature and component specs |
| 09 | milestones/ | Release summaries |
| 10 | PRs/ | Pull request history |
| 99 | meta/ | Templates (`00 templates/`) and vault maintenance |

## Structural Maintenance

Use the template repo's scripts to maintain vault health:

```bash
# Audit vault against canonical structure
/path/to/obsidian-docs-template/validate.sh .

# Preview structural fixes
/path/to/obsidian-docs-template/migrate.sh . --dry-run

# Apply fixes (additive only — never deletes content)
/path/to/obsidian-docs-template/migrate.sh .
```

Always run `validate.sh` after `migrate.sh` to confirm the result.

## Frontmatter Conventions

Notes use YAML frontmatter. Status-aware directories (architecture/adr, notes, specs) expect a `status` field:

```yaml
---
status: draft  # draft | approved | implemented | superseded | active | archived
tags: []
---
```

Index files live in their respective content directories (e.g. `01 concepts/CONCEPTS.md`) and use an `index` tag.

## Publishing Workflow

1. Write/edit in Obsidian
2. Export via Webpage HTML Export plugin (`Ctrl/Cmd+P` → "Export")
3. Commit the generated `docs/` directory
4. Push — GitHub Pages serves from `docs/`

The export path is configured to point at the project root's `docs/` directory.

## Things to Never Do

- Don't modify files under `.obsidian/plugins/` — these are managed by Obsidian and the template migration scripts
- Don't create numbered directories outside the canonical set without updating `lib/common.sh` in the template repo
- Don't delete `.gitkeep` files — they preserve empty directory structure in git
- Don't manually edit `.base` files — these are Obsidian-managed binary-ish JSON for database views
