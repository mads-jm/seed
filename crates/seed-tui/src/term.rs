/// Terminal capability probe and RAII raw-mode guard.
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use std::io::{self, Write};

/// RAII wrapper around crossterm raw mode + alternate screen.
/// `Drop` restores cooked mode and leaves the alternate screen unconditionally,
/// even on panic (via the panic hook installed in `main`).
pub struct TerminalGuard {
    active: bool,
}

impl TerminalGuard {
    /// Enter raw mode and alternate screen.
    pub fn enter() -> anyhow::Result<Self> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen)?;
        Ok(TerminalGuard { active: true })
    }

    /// Explicit restore (idempotent — safe to call before drop).
    pub fn restore(&mut self) {
        if self.active {
            self.active = false;
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), crossterm::cursor::Show, LeaveAlternateScreen);
            let _ = io::stdout().flush();
        }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        self.restore();
    }
}

/// Returns `true` if the terminal supports truecolor (24-bit RGB).
///
/// Probes `$COLORTERM in ("truecolor", "24bit")`.
/// `SEED_FORCE_256=1` overrides to false regardless of `$COLORTERM`.
pub fn truecolor_supported() -> bool {
    if std::env::var("SEED_FORCE_256").as_deref() == Ok("1") {
        return false;
    }
    matches!(
        std::env::var("COLORTERM").as_deref(),
        Ok("truecolor") | Ok("24bit")
    )
}

/// Returns `true` if braille characters are likely renderable.
///
/// Best-effort — assume yes unless `SEED_FORCE_ASCII=1`.
/// True font glyph coverage detection is not feasible from a TUI.
pub fn braille_supported() -> bool {
    std::env::var("SEED_FORCE_ASCII").as_deref() != Ok("1")
}
