//! The first-run guide, in both apps: four steps a new user goes through once (or skips), and
//! that F1 and Settings → Overview open again. 1: the two panels and the keys that matter most;
//! 2: how far Find looks (the search levels; meaning and Ask start the search setup guide); 3:
//! looks: theme, language, icons (whether a Nerd Font is there); 4: what can leave the machine,
//! and the update check. Each app draws the steps; what they show is decided here.

use crate::config::{Action, Config};
use crate::t;

pub const STEPS: usize = 4;

/// The four themes offered at once; the others are in Settings → Looks.
pub const THEMES: [&str; 4] = ["cyber", "nc", "midnight", "light"];

/// Step 1: the actions to know first, each with its key as configured.
pub fn keys(cfg: &Config) -> Vec<(String, String)> {
    [Action::SwitchPanel, Action::Open, Action::Copy, Action::Move, Action::Search, Action::Menu, Action::Help]
        .into_iter()
        .map(|a| (a.label(), cfg.key_for(a).unwrap_or("—").to_string()))
        .collect()
}

/// Step 1's line on the action menu and what a right-click does.
pub fn menu_line(cfg: &Config) -> String {
    t!("guide.menu_line", "key" => cfg.keys.get(&Action::ActionMenu).map(|k| k.join(" / ")).unwrap_or_default())
}

/// Step 1's line on F1 → Features.
pub fn features_line(cfg: &Config) -> String {
    t!("guide.features_line", "key" => cfg.key_for(Action::Help).unwrap_or("—"))
}

/// Its title: "Two panels · 1 of 4".
pub fn title(step: usize) -> String {
    let name = t!(&format!("guide.step{}", step + 1));
    t!("guide.title", "name" => name, "n" => step + 1, "of" => STEPS)
}

/// The guide was closed, done or skipped: not shown by itself again.
pub fn seen(state: &mut crate::state::AppState) {
    state.guide_seen = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guide_is_due_once_on_a_first_start() {
        let mut st = crate::state::AppState::default();
        assert!(st.guide_due());
        seen(&mut st);
        assert!(!st.guide_due());
        // Updated from 1.x: a version was started before, so what is new is told instead.
        let st = crate::state::AppState { seen_version: "1.44.0".into(), ..Default::default() };
        assert!(!st.guide_due());
        let k = keys(&Config::default());
        assert_eq!(k[2].1, "F5");
        assert!(title(0).contains('1') && title(0).contains('4'), "{}", title(0));
        assert!(THEMES.iter().all(|t| crate::config::Theme::builtin().iter().any(|(n, _)| n == t)));
    }
}
