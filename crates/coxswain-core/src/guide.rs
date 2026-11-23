//! The first-run guide, in both apps: four steps a new user goes through once (or skips), five
//! when podman or docker is found, and that F1 and Settings → Overview open again. 1: the two
//! panels and the keys that matter most; 2: how far Find looks (the search levels; meaning and
//! Ask start the search setup guide); 3: looks: theme, language, icons (whether a Nerd Font is
//! there); 5, only with a container tool: keeping the preview images up to date (off, check and
//! ask, weekly, monthly) and removing those not used for 90 days; 4: what can leave the machine,
//! and the update check, last, so it lists what the choices before it send. Each app draws the
//! steps; what they show is decided here.

use crate::config::{Action, Config};
use crate::t;

/// The steps shown, by number (1 to 5, as `guide.step<n>` names them), in order: the images step
/// (5) only when a container tool is found, before the last (4).
pub fn plan(cfg: &Config) -> Vec<usize> {
    let mut steps = vec![1, 2, 3];
    if crate::tools::container_runtime(&cfg.preview).is_some() {
        steps.push(5);
    }
    steps.push(4);
    steps
}

/// What the images step offers, as `[preview] image_updates` values.
pub const IMAGE_CHOICES: [&str; 4] = ["off", "ask", "weekly", "monthly"];

/// The four themes offered at once; the others are in Settings → Looks.
pub const THEMES: [&str; 4] = ["cyber", "nc", "midnight", "light"];

/// Step 1: the actions to know first, each with its key as configured.
pub fn keys(cfg: &Config) -> Vec<(String, String)> {
    [
        Action::SwitchPanel,
        Action::Open,
        Action::Copy,
        Action::Move,
        Action::Search,
        Action::SpaceView,
        Action::Menu,
        Action::Help,
    ]
    .into_iter()
    .map(|a| {
        (
            a.label(),
            cfg.key_for(a)
                .map_or_else(|| "—".into(), crate::config::written),
        )
    })
    .collect()
}

/// Step 1's line on the action menu and what a right-click does.
pub fn menu_line(cfg: &Config) -> String {
    t!("guide.menu_line", "key" => cfg.keys_shown(Action::ActionMenu))
}

/// Step 1's line on F1 → Features.
pub fn features_line(cfg: &Config) -> String {
    t!("guide.features_line", "key" => cfg.key_for(Action::Help).map_or_else(|| "—".into(), crate::config::written))
}

/// The title of the `at`th (from 0) of `plan`: "Two panels · 1 of 4".
pub fn title(plan: &[usize], at: usize) -> String {
    let name = t!(&format!("guide.step{}", plan[at]));
    t!("guide.title", "name" => name, "n" => at + 1, "of" => plan.len())
}

/// The guide was closed, done or skipped: not shown by itself again.
pub fn seen(state: &mut crate::state::AppState) {
    state.guide_seen = true;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::images::every;

    #[test]
    fn images_step_only_with_a_container_tool_and_before_privacy() {
        let mut cfg = Config::default();
        cfg.preview.container = "off".into();
        assert_eq!(plan(&cfg), [1, 2, 3, 4]);
        // Whatever this machine has, the privacy step is last and the images step before it.
        cfg.preview.container = "auto".into();
        let p = plan(&cfg);
        assert_eq!(p.last(), Some(&4));
        assert_eq!(p.contains(&5), crate::tools::container_runtime(&cfg.preview).is_some());
        assert!(IMAGE_CHOICES.iter().all(|c| every(c).is_some() || *c == "off"));
    }

    #[test]
    fn guide_is_due_once_on_a_first_start() {
        let mut st = crate::state::AppState::default();
        assert!(st.guide_due());
        seen(&mut st);
        assert!(!st.guide_due());
        // Updated from 1.x: a version was started before, so what is new is told instead.
        let st = crate::state::AppState {
            seen_version: "1.44.0".into(),
            ..Default::default()
        };
        assert!(!st.guide_due());
        let k = keys(&Config::default());
        assert_eq!(k[2].1, "F5");
        assert!(
            title(&[1, 2, 3, 4], 0).contains('1') && title(&[1, 2, 3, 4], 0).contains('4'),
            "{}",
            title(&[1, 2, 3, 4], 0)
        );
        assert!(
            THEMES
                .iter()
                .all(|t| crate::config::Theme::BUILTIN.iter().any(|(n, ..)| n == t))
        );
    }
}
