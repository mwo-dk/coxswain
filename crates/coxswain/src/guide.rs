//! The first-run guide in the terminal app (`coxswain_core::guide`): its four steps full screen
//! in the panel colours. ↑ ↓ choose, Space (or ← →) changes, Enter goes on, Backspace back, Esc
//! skips. Each change is written to config.toml at once, as in Settings.

use crate::ui::{dstyle, fit, frame, sty};
use crate::{App, Dialog};
use coxswain_core::config::Config;
use coxswain_core::guide::{self as cg, STEPS, THEMES};
use coxswain_core::settings::{self as cs, Level};
use coxswain_core::t;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;
use serde_json::{Map, Value};

#[derive(Default)]
pub struct Guide {
    pub step: usize,
    /// The row chosen in the step.
    pub cursor: usize,
    /// What the last change did: saved, copied, or why not.
    said: Option<(String, bool)>,
}

/// What a row of a step changes.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Row {
    Level(Level),
    Theme,
    Language,
    Icons,
    /// The line that installs a Nerd Font, to copy.
    Font,
    Updates,
}

fn rows(step: usize) -> Vec<Row> {
    match step {
        1 => Level::ALL.into_iter().map(Row::Level).collect(),
        2 => [Row::Theme, Row::Language, Row::Icons].into_iter().chain(coxswain_core::tools::install("nerd-font").map(|_| Row::Font)).collect(),
        3 => vec![Row::Updates],
        _ => vec![],
    }
}

/// `text` on the terminal's clipboard (OSC 52): the terminal puts it there, over ssh too.
pub fn copy(text: &str) {
    use std::io::Write;
    // In Termux, Android's clipboard through Termux:API when it is there.
    if coxswain_core::termux::copy(text) {
        return;
    }
    const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in text.as_bytes().chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            out.push(if i <= c.len() { B64[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    let _ = write!(std::io::stdout(), "\x1b]52;c;{out}\x07");
    let _ = std::io::stdout().flush();
}

/// The next (or, `back`, the previous) of `all` after `now`.
fn next<'a>(all: &[&'a str], now: &str, back: bool) -> &'a str {
    let i = all.iter().position(|x| *x == now).unwrap_or(all.len() - 1);
    all[if back { (i + all.len() - 1) % all.len() } else { (i + 1) % all.len() }]
}

impl App {
    /// A key in the guide.
    pub fn guide_key(&mut self, mut g: Box<Guide>, key: coxswain_core::config::Key, esc: bool) {
        use coxswain_core::config::KeyCode;
        let rows = rows(g.step);
        g.said = None;
        match key.code {
            _ if esc => return self.guide_close(),
            KeyCode::Enter if g.step + 1 >= STEPS => return self.guide_close(),
            KeyCode::Enter => (g.step, g.cursor) = (g.step + 1, 0),
            KeyCode::Backspace => (g.step, g.cursor) = (g.step.saturating_sub(1), 0),
            KeyCode::Up => g.cursor = g.cursor.saturating_sub(1),
            KeyCode::Down => g.cursor = (g.cursor + 1).min(rows.len().saturating_sub(1)),
            KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right => {
                if let Some(&row) = rows.get(g.cursor) {
                    return self.guide_change(g, row, key.code == KeyCode::Left);
                }
            }
            _ => {}
        }
        self.dialog = Some(Dialog::Guide(g));
    }

    /// Closed, done or skipped: not shown by itself again.
    fn guide_close(&mut self) {
        let mut st = coxswain_core::state::AppState::load();
        cg::seen(&mut st);
        // A test never writes the user's state.
        if !cfg!(test) {
            let _ = st.save();
        }
    }

    fn guide_change(&mut self, mut g: Box<Guide>, row: Row, back: bool) {
        let set = |name: &str, v: Value| Map::from_iter([(name.to_string(), v)]);
        let changes = match row {
            Row::Level(l) => match cs::level_changes(l, &self.cfg.search, coxswain_core::meaning::installed()) {
                Some(c) => c,
                // A model to choose first: the search setup guide, then back here.
                None => {
                    self.setup_guide();
                    return self.dialog = Some(Dialog::Guide(g));
                }
            },
            Row::Theme => set("tui_theme", next(&THEMES, &self.cfg.theme, back).into()),
            Row::Language => {
                let all: Vec<&str> = std::iter::once("auto").chain(coxswain_core::i18n::LANGUAGES.iter().map(|l| l.code)).collect();
                set("language", next(&all, &self.cfg.language, back).into())
            }
            Row::Icons => set("glyphs", if self.cfg.plain_glyphs() { "nerd" } else { "ascii" }.into()),
            Row::Updates => set("check_updates", (!self.cfg.check_updates).into()),
            Row::Font => {
                let line = coxswain_core::tools::install("nerd-font").unwrap_or_default();
                copy(&line);
                g.said = Some((t!("guide.copied", "command" => line), false));
                return self.dialog = Some(Dialog::Guide(g));
            }
        };
        if let Some(path) = Config::path() {
            g.said = self.save_options(&path, &changes).err().map(|e| (e, true));
        }
        self.dialog = Some(Dialog::Guide(g));
    }
}

/// Full screen, in the panel colours: the step's title, what it says, its rows, the keys.
pub fn draw(f: &mut Frame, app: &mut App) {
    let t = app.theme.clone();
    let Some(Dialog::Guide(g)) = &app.dialog else { return };
    let (step, cursor, said) = (g.step, g.cursor, g.said.clone());
    let inner = frame(f, app, f.area(), &cg::title(step));
    if inner.height < 6 || inner.width < 30 {
        return;
    }
    let base = dstyle(&t);
    let selected = sty(&t.dialog_input);
    let head = base.fg(sty(&t.header).fg.unwrap_or(Color::Reset)).add_modifier(Modifier::BOLD);
    let dim = base.add_modifier(Modifier::DIM);
    let red = base.fg(Color::Red);
    let w = inner.width.saturating_sub(4) as usize;
    let cfg = &app.cfg;
    let para = |s: String| Line::from(Span::raw(s));
    let mut lines: Vec<Line> = vec![];
    let row = |i: usize, text: String| Line::from(Span::styled(fit(&format!(" {text}"), w), if i == cursor { selected } else { base }));
    match step {
        0 => {
            lines.push(para(t!("guide.panels_intro")));
            lines.push(Line::from(""));
            for (label, key) in cg::keys(cfg) {
                lines.push(Line::from(vec![Span::styled(format!("  {key:<10}"), head), Span::raw(label)]));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(t!("guide.panels_more"), dim)));
        }
        1 => {
            lines.push(para(t!("guide.find_intro", "key" => app.key_label(coxswain_core::config::Action::Search))));
            lines.push(Line::from(""));
            let now = cs::level(&cfg.search);
            for (i, l) in Level::ALL.into_iter().enumerate() {
                let mark = if now == Some(l) { "(•)" } else { "( )" };
                lines.push(row(i, format!("{mark} {}", t!(&format!("settings.level.{}", l.id())))));
                lines.push(Line::from(Span::styled(fit(&format!("      {}", t!(&format!("settings.level.{}.hint", l.id()))), w), dim)));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(t!("guide.find_setup"), dim)));
        }
        2 => {
            lines.push(para(t!("guide.looks_intro")));
            lines.push(Line::from(""));
            let lang = if cfg.language == "auto" { t!("settings.language_auto") } else { coxswain_core::i18n::find(&cfg.language).map_or(cfg.language.clone(), |l| l.name.to_string()) };
            let theme = coxswain_core::i18n::tr(&format!("theme.{}", cfg.theme), &[]);
            let icons = if cfg.plain_glyphs() { t!("settings.glyphs_ascii") } else { t!("settings.glyphs_nerd") };
            lines.push(row(0, format!("{}: ‹ {theme} ›", t!("setting.tui_theme"))));
            lines.push(row(1, format!("{}: ‹ {lang} ›", t!("setting.language"))));
            lines.push(row(2, format!("{}: ‹ {icons} ›", t!("setting.glyphs"))));
            if let Some(line) = coxswain_core::tools::install("nerd-font") {
                lines.push(row(3, format!("{} {line}", t!("guide.icons_install"))));
            }
            lines.push(Line::from(""));
            // The terminal's font is the terminal's: the user can see whether it has the icons.
            if !cfg.plain_glyphs() {
                lines.push(Line::from(vec![Span::styled("  \u{f07b} \u{f15b}  ", head), Span::raw(t!("guide.icons_unknown"))]));
            }
            lines.push(Line::from(Span::styled(t!("guide.looks_more"), dim)));
        }
        _ => {
            lines.push(para(t!("guide.privacy_intro")));
            lines.push(Line::from(""));
            let mark = if cfg.check_updates { "[x]" } else { "[ ]" };
            lines.push(row(0, format!("{mark} {} (api.github.com)", t!("guide.privacy_update"))));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(t!("guide.privacy_now"), head)));
            let out = cs::outbound(cfg);
            if out.is_empty() {
                lines.push(para(format!("  {}", t!("settings.privacy_nothing"))));
            }
            for o in out {
                let local = if o.local { format!(" ({})", t!("settings.privacy_local")) } else { String::new() };
                lines.push(para(format!("  {} → {}{local}", o.what, o.to)));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(t!("guide.again_tui"), dim)));
        }
    }
    let body = Rect { x: inner.x + 2, y: inner.y + 1, width: inner.width.saturating_sub(4), height: inner.height.saturating_sub(3) };
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), body);
    let (line, st) = match said {
        Some((s, bad)) => (s, if bad { red } else { dim }),
        None => (t!(if step + 1 < STEPS { "guide.keys_tui" } else { "guide.keys_tui_last" }), dim),
    };
    f.render_widget(Paragraph::new(fit(&line, w)).style(st), Rect { x: inner.x + 2, y: inner.bottom() - 1, width: w as u16, height: 1 });
}

#[cfg(test)]
mod tests {
    use super::*;
    use coxswain_core::config::{Key, KeyCode};
    use coxswain_core::helper::Client;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn guide_goes_through_four_steps_and_skips() {
        let d = std::env::temp_dir();
        let cfg = Config { check_updates: false, language: "en-GB".into(), ..Config::default() };
        let index = Client::with(None, &cfg.search, |_| {});
        let mut app = App::with_index(cfg, d.clone(), d, index).unwrap();
        app.dialog = Some(Dialog::Guide(Box::default()));
        let key = |app: &mut App, code| app.dialog_key(Key::new(code, false, false, false));
        let screen = |app: &mut App| {
            let mut term = Terminal::new(TestBackend::new(80, 24)).unwrap();
            term.draw(|f| crate::ui::draw(f, app)).unwrap();
            let buf = term.backend().buffer().clone();
            (0..24).map(|y| (0..80).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>()).collect::<Vec<_>>().join("\n")
        };
        for n in 1..=STEPS {
            let scr = screen(&mut app);
            assert!(scr.contains(&format!("{n} of 4")), "{scr}");
            assert!(scr.contains("Esc"), "the keys are said:\n{scr}");
            if n < STEPS {
                key(&mut app, KeyCode::Enter);
            }
        }
        assert!(screen(&mut app).contains("ask GitHub"), "step 4: the update check");
        key(&mut app, KeyCode::Backspace);
        assert!(matches!(&app.dialog, Some(Dialog::Guide(g)) if g.step == 2));
        key(&mut app, KeyCode::Esc);
        assert!(app.dialog.is_none(), "Esc skips the rest");
        assert_eq!(next(&THEMES, "nc", false), "midnight");
        assert_eq!(next(&THEMES, "cyber", true), "light");
    }
}
