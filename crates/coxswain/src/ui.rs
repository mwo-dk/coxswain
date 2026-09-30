//! Drawing. Everything here reads `App`; the only writes are scroll offsets and hit areas.

use crate::{App, Dialog};
use coxswain_core::{t, tn};
use coxswain_core::config::{self, Action, Key, KeyCode};
use coxswain_core::git::Kind;
use coxswain_core::index::State;
use chrono::{Local, TimeZone};
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;
use std::str::FromStr;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub(crate) fn sty(s: &config::Style) -> Style {
    let color = |c: &str| (!c.is_empty()).then(|| Color::from_str(c).ok()).flatten();
    let mut out = Style::default();
    if let Some(c) = color(&s.fg) {
        out = out.fg(c);
    }
    if let Some(c) = color(&s.bg) {
        out = out.bg(c);
    }
    if s.bold {
        out = out.add_modifier(Modifier::BOLD);
    }
    out
}

/// Cut to `w` columns, marking the cut with `…`; pad to exactly `w`.
pub(crate) fn fit(s: &str, w: usize) -> String {
    if s.width() <= w {
        return format!("{s}{}", " ".repeat(w - s.width()));
    }
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if used + cw + 1 > w {
            break;
        }
        out.push(c);
        used += cw;
    }
    out.push('…');
    format!("{out}{}", " ".repeat(w.saturating_sub(used + 1)))
}

/// Keep the tail of a path, which is the informative part.
fn fit_left(s: &str, w: usize) -> String {
    if s.width() <= w {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut used = 1;
    let mut start = chars.len();
    while start > 0 && used + chars[start - 1].width().unwrap_or(0) <= w {
        start -= 1;
        used += chars[start].width().unwrap_or(0);
    }
    format!("…{}", chars[start..].iter().collect::<String>())
}

fn size(n: u64) -> String {
    if n < 100_000_000 {
        return n.to_string();
    }
    let mut v = n as f64;
    for unit in ["K", "M", "G", "T", "P"] {
        v /= 1024.0;
        if v < 10_000.0 {
            return format!("{v:.0}{unit}");
        }
    }
    format!("{v:.0}E")
}

fn date(secs: u64) -> String {
    Local.timestamp_opt(secs as i64, 0).single().map(|t| t.format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default()
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let [main, cmd, keys] = Layout::vertical([Constraint::Min(0), Constraint::Length(1), Constraint::Length(1)]).areas(f.area());
    let [l, r] = Layout::horizontal([Constraint::Percentage(50); 2]).areas(main);
    app.areas = [l, r];
    panel(f, app, 0, l);
    panel(f, app, 1, r);
    cmdline(f, app, cmd);
    keybar(f, app, keys);
    let theme = app.theme.clone();
    match &mut app.dialog {
        Some(Dialog::Bom(v)) => v.draw(f, &theme),
        Some(_) => dialog(f, app),
        None => {}
    }
}

fn panel(f: &mut Frame, app: &mut App, side: usize, area: Rect) {
    let t = &app.theme;
    let active = side == app.active && app.dialog.is_none();
    let (base, border) = (sty(&t.panel), sty(&t.border).bg(sty(&t.panel).bg.unwrap_or(Color::Reset)));
    let p = &app.panels[side];

    let title_style = if side == app.active { sty(&t.cursor) } else { border };
    // Inside an archive the title says so, so a copy out is not taken for one between folders.
    let dir = match coxswain_core::archive::split(&p.dir) {
        Some(_) => format!("{} [{}]", p.dir.to_string_lossy(), t!("archive.badge")),
        None => p.dir.to_string_lossy().into_owned(),
    };
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(border)
        .style(base)
        .title(Line::from(Span::styled(format!(" {} ", fit_left(&dir, area.width.saturating_sub(6) as usize)), title_style)).centered());
    if let Some(g) = &p.git {
        let prompt = g.prompt(&app.glyphs);
        block = block.title_bottom(Line::from(Span::styled(format!(" {prompt} "), sty(&t.git_branch))).left_aligned());
    }
    let sort = match p.sort {
        coxswain_core::fs::SortKey::Name => "n",
        coxswain_core::fs::SortKey::Ext => "x",
        coxswain_core::fs::SortKey::Time => "t",
        coxswain_core::fs::SortKey::Size => "s",
    };
    let sort = if p.reverse { sort.to_uppercase() } else { sort.to_string() };
    block = block.title_bottom(Line::from(Span::styled(format!(" {sort} "), border)).right_aligned());
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height < 4 || inner.width < 12 {
        return;
    }

    // Columns: git glyph | name | size | date
    let w = inner.width as usize;
    let show_date = w >= 44;
    let (gw, sw, dw) = (2, 9, if show_date { 16 } else { 0 });
    let nw = w - gw - sw - 1 - if show_date { dw + 1 } else { 0 };
    let bar = Span::styled("│", border);

    let header = {
        let hs = sty(&t.header).bg(base.bg.unwrap_or(Color::Reset));
        let mut v = vec![Span::styled(" ".repeat(gw), hs), Span::styled(fit(&format!("{:^nw$}", t!("tui.col.name")), nw), hs), bar.clone(), Span::styled(fit(&format!("{:^sw$}", t!("tui.col.size")), sw), hs)];
        if show_date {
            v.push(bar.clone());
            v.push(Span::styled(fit(&format!("{:^dw$}", t!("tui.col.modified")), dw), hs));
        }
        Line::from(v)
    };
    f.render_widget(Paragraph::new(header), Rect { height: 1, ..inner });

    let rows = inner.height as usize - 3;
    let p = &mut app.panels[side];
    p.page = rows;
    if p.cursor < p.offset {
        p.offset = p.cursor;
    } else if p.cursor >= p.offset + rows {
        p.offset = p.cursor + 1 - rows;
    }
    p.offset = p.offset.min(p.entries.len().saturating_sub(rows));
    let p = &app.panels[side];

    let mut lines = vec![];
    for (i, e) in p.entries.iter().enumerate().skip(p.offset).take(rows) {
        let marked = p.marked.contains(&e.path);
        let at = i == p.cursor && active;
        let mut s = if marked {
            sty(&t.marked)
        } else if e.is_dir {
            sty(&t.directory)
        } else if e.is_symlink {
            sty(&t.symlink)
        } else if e.is_exec {
            sty(&t.executable)
        } else if e.hidden {
            sty(&t.hidden)
        } else {
            base
        };
        s = s.bg(base.bg.unwrap_or(Color::Reset));
        if at {
            s = if marked { sty(&t.marked_cursor) } else { sty(&t.cursor) };
        }
        let row_bg = s.bg.unwrap_or(Color::Reset);
        let (glyph, gstyle) = match p.git.as_ref().and_then(|g| g.get(&e.path)).filter(|_| !e.is_parent()) {
            Some(st) => {
                let gs = match st.kind {
                    Kind::Modified => &t.git_modified,
                    Kind::Added => &t.git_added,
                    Kind::Untracked => &t.git_untracked,
                    Kind::Deleted => &t.git_deleted,
                    Kind::Renamed => &t.git_renamed,
                    Kind::Conflict => &t.git_conflict,
                    Kind::Ignored => &t.git_ignored,
                };
                (st.kind.glyph(&app.glyphs).to_string(), sty(gs).bg(row_bg))
            }
            None => (String::new(), s),
        };
        let name = if app.cfg.glyphs == "ascii" || e.is_parent() {
            format!("  {}", e.name)
        } else {
            format!("{} {}", coxswain_core::icons::icon(&e.name, e.is_dir).glyph, e.name)
        };
        let name = &name;
        let sz = if e.is_parent() {
            t!("tui.updir")
        } else if let Some(n) = p.sizes.get(&e.path) {
            size(*n)
        } else if e.is_dir {
            t!("tui.subdir")
        } else {
            size(e.size)
        };
        let bar = Span::styled("│", border.bg(row_bg).fg(border.fg.unwrap_or(Color::Reset)));
        let mut v = vec![Span::styled(fit(&glyph, gw), gstyle), Span::styled(fit(name, nw), s), bar.clone(), Span::styled(format!("{sz:>sw$}"), s)];
        if show_date {
            v.push(bar);
            v.push(Span::styled(fit(&if e.is_parent() { String::new() } else { date(e.modified) }, dw), s));
        }
        lines.push(Line::from(v));
    }
    // Keep the column bars running to the bottom.
    while lines.len() < rows {
        let mut v = vec![Span::raw(" ".repeat(gw + nw)), bar.clone(), Span::raw(" ".repeat(sw))];
        if show_date {
            v.push(bar.clone());
        }
        lines.push(Line::from(v));
    }
    f.render_widget(Paragraph::new(lines), Rect { y: inner.y + 1, height: rows as u16, ..inner });

    // Separator and info line.
    let sep_y = inner.y + 1 + rows as u16;
    f.render_widget(Paragraph::new(Span::styled("─".repeat(w), border)), Rect { y: sep_y, height: 1, ..inner });
    let info = if let Some(err) = &p.error {
        Line::from(Span::styled(fit(err, w), sty(&t.git_conflict)))
    } else if !p.marked.is_empty() {
        let bytes: u64 = p.entries.iter().filter(|e| p.marked.contains(&e.path)).map(|e| e.size).sum();
        Line::from(Span::styled(format!("{:^w$}", tn!("tui.selected", p.marked.len(), "size" => size(bytes))), sty(&t.marked))).centered()
    } else if let Some(e) = p.current() {
        let right = if e.is_dir { String::new() } else { format!(" {}", size(e.size)) };
        let mut name = e.name.clone();
        if e.is_symlink {
            if let Ok(target) = std::fs::read_link(&e.path) {
                name = format!("{name} -> {}", target.display());
            }
        }
        Line::from(vec![Span::styled(fit(&name, w.saturating_sub(right.width())), base), Span::styled(right, base)])
    } else {
        Line::default()
    };
    f.render_widget(Paragraph::new(info), Rect { y: sep_y + 1, height: 1, ..inner });
}

fn cmdline(f: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let st = sty(&t.cmdline);
    let text = if let Some(q) = &app.quick {
        t!("quick_search", "query" => q)
    } else if let Some(s) = &app.status {
        if app.cmdline.is_empty() { s.clone() } else { String::new() }
    } else {
        String::new()
    };
    let (line, cursor) = if !text.is_empty() {
        (Line::from(Span::styled(fit(&text, area.width as usize), st)), None)
    } else {
        let prompt = format!("{}> ", fit_left(&app.panel().dir.to_string_lossy(), area.width as usize / 2));
        let x = area.x + (prompt.width() + app.cmdline.width()) as u16;
        let full = format!("{prompt}{}", app.cmdline);
        (Line::from(Span::styled(fit_left(&full, area.width as usize), st)), Some(x.min(area.right() - 1)))
    };
    f.render_widget(Paragraph::new(line).style(st), area);
    if let (Some(x), None) = (cursor, &app.dialog) {
        f.set_cursor_position(Position::new(x, area.y));
    }
}

fn keybar(f: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    let slot = area.width as usize / 10;
    let mut spans = vec![];
    for n in 1..=10u8 {
        let key = Key::new(KeyCode::F(n), false, false, false);
        let label = Action::ALL
            .iter()
            .find(|&&a| app.cfg.keys.get(&a).is_some_and(|ks| ks.iter().any(|k| k.parse::<Key>().ok() == Some(key))))
            .map_or(String::new(), |a| a.label());
        let num = n.to_string();
        let lw = slot.saturating_sub(num.len());
        spans.push(Span::styled(num, sty(&t.keybar_num)));
        spans.push(Span::styled(fit(&label, lw), sty(&t.keybar_label)));
    }
    f.render_widget(Paragraph::new(Line::from(spans)).style(sty(&t.keybar_num)), area);
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h }
}

fn frame(f: &mut Frame, app: &App, area: Rect, title: &str) -> Rect {
    let t = &app.theme;
    let bg = sty(&t.dialog);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(sty(&t.dialog_border).bg(bg.bg.unwrap_or(Color::Reset)))
        .style(bg)
        .title(Line::from(format!(" {title} ")).centered());
    f.render_widget(Clear, area);
    let inner = block.inner(area);
    f.render_widget(block, area);
    inner
}

fn input_line(f: &mut Frame, app: &App, area: Rect, value: &str) {
    let w = area.width as usize;
    let shown = fit_left(value, w.saturating_sub(1));
    f.render_widget(Paragraph::new(fit(&shown, w)).style(sty(&app.theme.dialog_input)), area);
    f.set_cursor_position(Position::new(area.x + shown.width() as u16, area.y));
}

fn dialog(f: &mut Frame, app: &mut App) {
    let full = f.area();
    let t = app.theme.clone();
    let dstyle = sty(&t.dialog);
    match app.dialog.as_ref().unwrap() {
        Dialog::Input { title, label, value, prompt } => {
            let inner = frame(f, app, centered(full, 70, 6), title);
            let [a, b, _, c] = Layout::vertical([Constraint::Length(1); 4]).areas(inner.inner(ratatui::layout::Margin::new(1, 0)));
            f.render_widget(Paragraph::new(label.as_str()), a);
            // A password shows as stars.
            let shown = if matches!(prompt, crate::Prompt::Password(..) | crate::Prompt::Unlock(..)) { "*".repeat(value.chars().count()) } else { value.clone() };
            input_line(f, app, b, &shown);
            f.render_widget(Paragraph::new(t!("tui.ok_cancel")).centered(), c);
        }
        Dialog::Confirm { title, text, .. } => {
            let inner = frame(f, app, centered(full, 60, 6), title);
            let [a, _, b] = Layout::vertical([Constraint::Length(2), Constraint::Length(1), Constraint::Length(1)]).areas(inner);
            f.render_widget(Paragraph::new(text.as_str()).centered().wrap(Wrap { trim: true }), a);
            f.render_widget(Paragraph::new(t!("tui.yes_no")).centered(), b);
        }
        Dialog::Message { title, text } => {
            let h = (text.lines().count() as u16 + 4).min(full.height);
            let inner = frame(f, app, centered(full, 76, h), title);
            f.render_widget(Paragraph::new(text.as_str()).wrap(Wrap { trim: false }), inner);
        }
        Dialog::Help { scroll } => {
            let area = centered(full, 72, full.height.saturating_sub(4));
            let inner = frame(f, app, area, &t!("help.title"));
            f.render_widget(Paragraph::new(help_text(app)).scroll((*scroll, 0)), inner.inner(ratatui::layout::Margin::new(1, 0)));
        }
        Dialog::Menu { title, filter, items, cursor, direct } => {
            let visible: Vec<_> = items.iter().filter(|it| it.label.to_lowercase().contains(&filter.to_lowercase())).collect();
            let kw = items.iter().map(|i| i.key.width()).max().unwrap_or(0);
            let lw = items.iter().map(|i| i.label.width()).max().unwrap_or(0);
            let h = (visible.len() as u16 + 2 + !direct as u16).min(full.height - 2);
            let inner = frame(f, app, centered(full, (kw + lw + 6) as u16, h), title);
            let mut y = inner.y;
            if !direct {
                input_line(f, app, Rect { y, height: 1, ..inner }, filter);
                y += 1;
            }
            let rows = (inner.bottom() - y) as usize;
            let skip = cursor.saturating_sub(rows.saturating_sub(1));
            for (i, it) in visible.iter().enumerate().skip(skip).take(rows) {
                let s = if i == *cursor { sty(&t.dialog_input) } else { dstyle };
                let line = format!(" {} {}", fit(&it.label, lw), fit(&it.key, kw));
                f.render_widget(Paragraph::new(fit(&line, inner.width as usize)).style(s), Rect { y, height: 1, ..inner });
                y += 1;
            }
        }
        Dialog::Search { .. } => search(f, app, full),
        Dialog::Bom(_) => {}
    }
}

fn search(f: &mut Frame, app: &mut App, full: Rect) {
    let t = app.theme.clone();
    let area = centered(full, full.width.saturating_sub(8).max(40), full.height.saturating_sub(4));
    let inner = frame(f, app, area, &t!("search.title"));
    let dir = app.panel().dir.to_string_lossy().into_owned();
    let now = app.index.status();
    let (state, count) = (now.state, now.len);
    let Some(Dialog::Search { query, mode, results, cursor, offset }) = &mut app.dialog else { return };
    let text = *mode == 2;
    let [q, info, list, help] = Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)]).areas(inner);
    let prompt = match *mode {
        2 => t!("search.text"),
        1 => t!("search.in", "dir" => fit_left(&dir, 30)),
        _ => t!("search.everywhere"),
    };
    f.render_widget(Paragraph::new(prompt.as_str()), q);
    let qa = Rect { x: q.x + prompt.width() as u16, width: q.width.saturating_sub(prompt.width() as u16), ..q };
    let w = qa.width as usize;
    let shown = fit_left(query, w.saturating_sub(1));
    f.render_widget(Paragraph::new(fit(&shown, w)).style(sty(&t.dialog_input)), qa);
    f.set_cursor_position(Position::new(qa.x + shown.width() as u16, qa.y));

    let st = match state {
        State::Stale => t!("search.refreshing"),
        State::Building => t!("search.building"),
        State::Ready => String::new(),
    };
    let (indexed, st) = if text {
        (tn!("search.texts", now.texts), if now.pending > 0 { t!("search.reading", "n" => now.pending) } else { String::new() })
    } else {
        (tn!("search.indexed", count), st)
    };
    let msg = if query.is_empty() && text {
        format!("{indexed}{st} · {}", t!("dialogs.text_hint"))
    } else if query.is_empty() {
        format!("{indexed}{st} · {}", t!("dialogs.search_tab_hint"))
    } else {
        let ms = format!("{:.2}", results.micros as f64 / 1000.0);
        format!("{} · {indexed}{st}", tn!("search.matches", results.total, "ms" => ms))
    };
    f.render_widget(Paragraph::new(msg), info);
    // Search by meaning is off: say it is there, and how it is turned on.
    let list = if text && !now.meaning && results.hits.is_empty() {
        let [tip, rest] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(list);
        f.render_widget(Paragraph::new(t!("tui.meaning_tip")).style(dstyle(&t).add_modifier(Modifier::DIM)), tip);
        rest
    } else {
        list
    };

    // A hit in the text takes two lines: the file, and the passage that matched.
    let rows = list.height as usize / if text { 2 } else { 1 };
    if *cursor < *offset {
        *offset = *cursor;
    } else if *cursor >= *offset + rows {
        *offset = *cursor + 1 - rows;
    }
    let hit_style = sty(&t.search_hit);
    let lines: Vec<Line> = results
        .hits
        .iter()
        .enumerate()
        .skip(*offset)
        .take(rows)
        .flat_map(|(i, h)| {
            let base = if i == *cursor { sty(&t.dialog_input) } else { dstyle(&t) };
            let p = h.path.to_string_lossy();
            let (parent, name) = p.rsplit_once(std::path::MAIN_SEPARATOR).unwrap_or(("", &p));
            let name = if h.is_dir { format!("{name}{}", std::path::MAIN_SEPARATOR) } else { name.to_string() };
            let pw = (list.width as usize).saturating_sub(name.width() + 2);
            let file = Line::from(vec![
                Span::styled(format!(" {name} "), base.patch(hit_style).bg(base.bg.unwrap_or(Color::Reset))),
                Span::styled(fit(&fit_left(parent, pw), pw), base),
            ]);
            let Some(snippet) = h.snippet.as_deref().filter(|_| text) else { return vec![file] };
            // The words found stand out; the rest is dim.
            let dim = dstyle(&t).add_modifier(Modifier::DIM);
            let mut passage = vec![Span::styled("   ", dim)];
            // Found by meaning: the passage that was close, after what it is.
            let label = h.similar.map(|_| format!("{} ", t!("search.similar_to"))).unwrap_or_default();
            if !label.is_empty() {
                passage.push(Span::styled(label.clone(), dstyle(&t).patch(hit_style)));
            }
            for (n, part) in fit(snippet, (list.width as usize).saturating_sub(3 + label.width())).split([coxswain_core::store::MARK.0, coxswain_core::store::MARK.1]).enumerate() {
                passage.push(Span::styled(part.to_string(), if n % 2 == 1 { dstyle(&t).patch(hit_style) } else { dim }));
            }
            vec![file, Line::from(passage)]
        })
        .collect();
    f.render_widget(Paragraph::new(lines), list);
    f.render_widget(
        Paragraph::new(t!("tui.search_footer")).centered(),
        help,
    );
}

fn dstyle(t: &config::Theme) -> Style {
    sty(&t.dialog)
}

fn help_text(app: &App) -> Vec<Line<'static>> {
    let mut v = vec![
        Line::from(t!("help.tagline", "version" => coxswain_core::update::VERSION)).bold(),
        Line::from(""),
        Line::from(t!("help.keys")).bold(),
    ];
    for &a in Action::ALL.iter().filter(|a| !a.gui_only()) {
        let keys = app.cfg.keys.get(&a).map(|k| k.join(", ")).unwrap_or_default();
        v.push(Line::from(format!("  {:<22} {keys}", a.label())));
    }
    v.push(Line::from(""));
    for k in ["help.also1", "help.also2", "help.also3", "help.bom"] {
        v.push(Line::from(t!(k)));
    }
    v.push(Line::from(""));
    v.push(Line::from(t!("help.syntax")));
    // One query per line, so translations of any length line up.
    for (q, k) in [
        ("foo bar", "both"),
        ("foo|bar", "either"),
        ("!foo", "not"),
        ("*.rs  a?c", "wildcards"),
        ("ext:rs;toml", "ext"),
        ("file: folder:", "kind"),
        ("src/ foo", "path"),
        ("case:", "case"),
        ("\"a b\"", "phrase"),
    ] {
        v.push(Line::from(format!("  {q:<14} {}", t!(&format!("syntax.{k}")))));
    }
    v.push(Line::from(""));
    v.push(Line::from(t!("help.config", "path" => config::Config::path().map(|p| p.display().to_string()).unwrap_or_default())));
    v.push(Line::from(t!("help.dump")));
    v
}
