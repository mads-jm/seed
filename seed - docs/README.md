# seed Documentation Vault

This is an [Obsidian](https://obsidian.md) vault containing all project documentation. It follows a numbered Zettelkasten-inspired directory structure for consistent knowledge organization.

## Directory Structure

| # | Directory | Purpose |
|---|-----------|---------|
| 00 | `index/` | Navigation hub — index files that link into each category |
| 01 | `concepts/` | Atomic concept notes — durable, reusable knowledge |
| 02 | `references/` | API references and external documentation |
| 03 | `guides/` | Developer workflow and implementation guides |
| 04 | `architecture/` | Architecture design docs and ADRs |
| 05 | `notes/` | Fleeting development notes and working drafts |
| 06 | `reports/` | Sprint reports and progress snapshots |
| 07 | `stories/` | Vision documents and project narratives |
| 08 | `specs/` | Feature and component specifications |
| 09 | `milestones/` | Release and milestone summaries |
| 10 | `PRs/` | Pull request history |
| 99 | `meta/` | Templates and vault maintenance |

## Publishing

Documentation is published to GitHub Pages via the [Webpage HTML Export](https://github.com/KosmosisDire/obsidian-webpage-export) plugin. Export is triggered manually from within Obsidian — this is intentional, providing a human-in-the-loop review step before publication.

Exported HTML is written to the `docs/` directory at the project root, which GitHub Pages serves.
