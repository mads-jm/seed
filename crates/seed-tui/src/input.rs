/// crossterm event → Action mapping.
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// UI actions derived from terminal input events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Quit the TUI.
    Quit,
    /// Append a character to the command bar.
    Char(char),
    /// Delete last character from command bar.
    Backspace,
    /// Clear command bar.
    ClearInput,
    /// Toggle tweaks panel (Ctrl+T).
    ToggleTweaks,
    /// Cycle side panel tab.
    NextTab,
    /// Terminal was resized.
    Resize(u16, u16),
    /// Move selection up in the side panel (only when command bar is empty).
    ArrowUp,
    /// Move selection down in the side panel (only when command bar is empty).
    ArrowDown,
    /// Scroll up a page in the side panel.
    PageUp,
    /// Scroll down a page in the side panel.
    PageDown,
    /// Space key — pin/activate selected row or close overlay.
    /// Emitted unconditionally; app decides based on command-bar state.
    Space,
    /// Enter key on a selected row (opens detail or completes reminder).
    /// Distinct from Submit so the app can intercept when a row is selected.
    RowEnter,
    /// Toggle enabled state for the selected reminder (key `e`).
    ToggleEnabledKey,
    /// Ignored event.
    Noop,
}

/// Map a crossterm `Event` to a TUI `Action`.
///
/// Key events are filtered to `KeyEventKind::Press` and `KeyEventKind::Repeat` only.
/// Windows terminals emit both Press and Release for every keystroke — without
/// this filter, every key fires twice (leading to duplicate input).
///
/// Focus model: bare letter keys always type into the command bar. Sidebar row
/// actions that would conflict with reminder verbs (e.g. `e` colliding with
/// `eat`) require Ctrl. Bare `Enter`/`Space` still act on the sidebar when the
/// bar is empty since neither produces a meaningful command in that state; the
/// caller (App) decides based on `command.is_empty()`.
pub fn map_event(event: Event) -> Action {
    match event {
        Event::Key(KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press | KeyEventKind::Repeat,
            ..
        }) => map_key(code, modifiers),
        Event::Key(_) => Action::Noop, // ignore Release
        Event::Resize(w, h) => Action::Resize(w, h),
        _ => Action::Noop,
    }
}

fn map_key(code: KeyCode, modifiers: KeyModifiers) -> Action {
    if modifiers.contains(KeyModifiers::CONTROL) {
        return match code {
            KeyCode::Char('c') | KeyCode::Char('q') => Action::Quit,
            KeyCode::Char('t') => Action::ToggleTweaks,
            KeyCode::Char('e') => Action::ToggleEnabledKey,
            _ => Action::Noop,
        };
    }

    match code {
        // Note: bare 'q' is NOT Quit — it would conflict with the command bar
        // (e.g. typing "quit" or any verb starting with q). Use Ctrl+C / Ctrl+Q.
        KeyCode::Enter => Action::RowEnter,
        KeyCode::Tab => Action::NextTab,
        KeyCode::Backspace => Action::Backspace,
        KeyCode::Esc => Action::ClearInput,
        KeyCode::Up => Action::ArrowUp,
        KeyCode::Down => Action::ArrowDown,
        KeyCode::PageUp => Action::PageUp,
        KeyCode::PageDown => Action::PageDown,
        // Bare Space: nav-on-empty / type-into-bar otherwise. App decides.
        KeyCode::Char(' ') => Action::Space,
        // All other chars (including '/' and 'e') go straight to the command bar.
        // Sidebar toggle-enabled is Ctrl+E (handled above).
        KeyCode::Char(c) => Action::Char(c),
        _ => Action::Noop,
    }
}
