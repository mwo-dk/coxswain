//! Drawing. Everything here reads `App`; the only writes are scroll offsets and hit areas.

use crate::{App, Dialog};
use coxswain_core::{t, tn};
use coxswain_core::config::{self, Action, Key, KeyCode};
use coxswain_core::git::Kind;
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

/// `s` centred in `w` columns, by display width: a CJK letter takes two, which `{:^w$}` miscounts.
fn center(s: &str, w: usize) -> String {
    let gap = w.saturating_sub(s.width());
    format!("{}{s}{}", " ".repeat(gap / 2), " ".repeat(gap - gap / 2))
}

/// `s` against the right edge of `w` columns, cut to fit.
fn right(s: &str, w: usize) -> String {
    fit(&format!("{}{s}", " ".repeat(w.saturating_sub(s.width()))), w)
}

/// Keep the tail of a path, which is the informative part.
pub(crate) fn fit_left(s: &str, w: usize) -> String {
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
        Some(Dialog::Settings(_)) => crate::settings::draw(f, app),
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
    // So is a history, with the commit looked into.
    let dir = match (coxswain_core::archive::split(&p.dir), coxswain_core::history::split(&p.dir)) {
        (Some(_), _) => format!("{} [{}]", p.dir.to_string_lossy(), t!("archive.badge")),
        (_, Some(at)) => match (at.commit, at.view) {
            (Some(c), _) => format!("{} [{}]", p.dir.to_string_lossy(), t!("history.badge_at", "commit" => &c[..c.len().min(7)])),
            (None, coxswain_core::history::View::History) => format!("{} [{}]", p.dir.to_string_lossy(), t!("history.badge")),
            (None, coxswain_core::history::View::Branches) => format!("{} [{}]", p.dir.to_string_lossy(), t!("branches.badge")),
            (None, coxswain_core::history::View::Worktrees) => format!("{} [{}]", p.dir.to_string_lossy(), t!("worktrees.badge")),
        },
        _ => p.dir.to_string_lossy().into_owned(),
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
        coxswain_core::fs::SortKey::Commit => "c",
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
        let mut v = vec![Span::styled(" ".repeat(gw), hs), Span::styled(fit(&center(&t!("tui.col.name"), nw), nw), hs), bar.clone(), Span::styled(fit(&center(&t!("tui.col.size"), sw), sw), hs)];
        if show_date {
            v.push(bar.clone());
            v.push(Span::styled(fit(&center(&t!("tui.col.modified"), dw), dw), hs));
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

    let plain = app.cfg.plain_glyphs().then_some(&app.glyphs);
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
            // Only in the cloud: the glyph says so, and nothing reads it unasked.
            None if e.online => (app.glyphs.cloud.clone(), s),
            None => (String::new(), s),
        };
        let name = if e.is_parent() {
            format!("  {}", e.name)
        } else {
            format!("{} {}", coxswain_core::icons::entry(&e.name, e.is_dir, e.is_symlink, plain).glyph, e.name)
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
        let mut v = vec![Span::styled(fit(&glyph, gw), gstyle), Span::styled(fit(name, nw), s), bar.clone(), Span::styled(right(&sz, sw), s)];
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
        Line::from(Span::styled(fit(&center(&tn!("tui.selected", p.marked.len(), "size" => size(p.marked_bytes)), w), w), sty(&t.marked)))
    } else if let Some(e) = p.current() {
        let mut right = if e.is_dir { String::new() } else { format!(" {}", size(e.size)) };
        // The last commit that changed it, when git has said.
        match p.last.as_ref().and_then(|l| l.get(&e.name)) {
            Some(Some(c)) => right = format!(" {} {} {}{right}", c.hash, date(c.time), c.author),
            Some(None) => right = format!(" {}{right}", t!("history.older")),
            None => {}
        }
        if e.online {
            right = format!(" {}{right}", t!("tui.online", "key" => app.key_label(coxswain_core::config::Action::View)));
        }
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
        (Line::from(Span::styled(fit_left(&full, area.width as usize), st)), Some(x.min(area.right().saturating_sub(1))))
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
        // The keymap, so the bar names the action the key runs when two share it.
        let label = app.keymap.get(&Key::new(KeyCode::F(n), false, false, false)).map_or(String::new(), |a| a.label());
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

pub(crate) fn frame(f: &mut Frame, app: &App, area: Rect, title: &str) -> Rect {
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
            let shown = if matches!(prompt, crate::Prompt::Password(..) | crate::Prompt::Unlock(..) | crate::Prompt::Peek(..) | crate::Prompt::PackPassword(..) | crate::Prompt::PackConfirm(..)) { "*".repeat(value.chars().count()) } else { value.clone() };
            input_line(f, app, b, &shown);
            f.render_widget(Paragraph::new(t!("tui.ok_cancel")).centered(), c);
        }
        Dialog::Confirm { title, text, .. } | Dialog::Switch { title, text, .. } => {
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
            let area = centered(full, 120, full.height.saturating_sub(4));
            let inner = frame(f, app, area, &t!("help.title")).inner(ratatui::layout::Margin::new(1, 0));
            let text = help_text(app, inner.width as usize);
            f.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }).scroll((*scroll, 0)), inner);
        }
        Dialog::Menu { title, filter, items, cursor, direct } => {
            let visible: Vec<_> = items.iter().filter(|it| it.label.to_lowercase().contains(&filter.to_lowercase())).collect();
            let kw = items.iter().map(|i| i.key.width()).max().unwrap_or(0);
            let lw = items.iter().map(|i| i.label.width()).max().unwrap_or(0);
            let h = (visible.len() as u16 + 2 + !direct as u16).min(full.height.saturating_sub(2));
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
        Dialog::Bom(_) | Dialog::Settings(_) => {}
    }
}

fn search(f: &mut Frame, app: &mut App, full: Rect) {
    use coxswain_core::find::{self, Kind, Row};
    let t = app.theme.clone();
    let area = centered(full, full.width.saturating_sub(8).max(40), full.height.saturating_sub(4));
    let inner = frame(f, app, area, &t!("search.title"));
    let dir = app.panel().dir.clone();
    let now = app.index.status();
    let Some(Dialog::Search { query, chip, here, found, cursor, show, .. }) = &app.dialog else { return };
    let (query, chip, here, show) = (query.clone(), *chip, *here, *show);
    let rows = app.find_rows(&query, chip, found);
    let (prefix, question) = find::parse(&query);
    let kind = prefix.unwrap_or(chip);
    let answer = show == crate::Show::Answer || kind == Kind::Ask;
    let [q, chips, list, foot, keys] = Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(0), Constraint::Length(1), Constraint::Length(1)]).areas(inner);
    let dim = dstyle(&t).add_modifier(Modifier::DIM);
    let hit_style = sty(&t.search_hit);
    let folder = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| dir.display().to_string());

    // The field, and the scope at the right.
    let prompt = if answer { t!("search.ask") } else { t!("find.prompt") };
    let scope = format!(" [{}]", if here { t!("dialogs.scope_in", "folder" => fit_left(&folder, 30)) } else { t!("dialogs.scope_everywhere") }.to_lowercase());
    f.render_widget(Paragraph::new(prompt.as_str()), q);
    let w = (q.width as usize).saturating_sub(prompt.width() + scope.width());
    let qa = Rect { x: q.x + prompt.width() as u16, width: w as u16, ..q };
    let shown = fit_left(&query, w.saturating_sub(1));
    f.render_widget(Paragraph::new(fit(&shown, w)).style(sty(&t.dialog_input)), qa);
    f.render_widget(Paragraph::new(scope).style(dstyle(&t).patch(hit_style)), Rect { x: qa.x + w as u16, width: q.width.saturating_sub(prompt.width() as u16 + w as u16), ..q });
    f.set_cursor_position(Position::new(qa.x + shown.width() as u16, qa.y));

    // The kinds, the current one marked.
    let mut spans = vec![];
    for k in Kind::EVERY {
        let label = format!(" {} ", k.label());
        spans.push(if k == if answer { Kind::Ask } else { kind } { Span::styled(label, sty(&t.dialog_input)) } else { Span::styled(label, dstyle(&t)) });
        spans.push(Span::raw(" "));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), chips);
    f.render_widget(Paragraph::new(fit(&find::footer(&now), foot.width as usize)).style(dim), foot);
    let scope_key = app.key_label(Action::Search).to_string();
    let keys_text = if answer { t!("find.keys_answer_tui") } else { t!("find.keys_tui", "scope" => scope_key) };
    f.render_widget(Paragraph::new(fit(&keys_text, keys.width as usize)).centered(), keys);

    if show == crate::Show::Syntax {
        return f.render_widget(Paragraph::new(syntax_lines()).wrap(Wrap { trim: false }), list);
    }
    if answer {
        let (cursor, off) = (*cursor, app.ask_state().err());
        return ask(f, &app.chat, app.ask_rx.is_some(), off.as_ref(), &app.cfg.search, &t, cursor, list);
    }
    let width = list.width as usize;
    if question.trim().is_empty() {
        let lines = vec![Line::from(t!("find.empty"))];
        return f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), list);
    }
    if rows.is_empty() {
        let q = question.trim();
        let mut lines = vec![Line::from(if here { t!("find.nothing_in", "query" => q, "folder" => folder) } else { t!("find.nothing", "query" => q) })];
        if here {
            lines.push(Line::from(Span::styled(t!("find.try_everywhere", "key" => app.key_label(Action::Search)), dim)));
        }
        if find::asks(q) && app.ask_state().is_ok() {
            lines.push(Line::from(Span::styled(t!("find.try_ask", "key" => "Alt+Enter"), dim)));
        }
        return f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), list);
    }

    // Each row as lines; a hit with a passage takes two.
    let step_hint = |off: &find::Off| {
        let mut s = format!(" · Enter: {}", off.step());
        if off.dismiss_id().is_some() {
            s += &format!(" · {}", t!("find.dismiss"));
        }
        s
    };
    let header_fg = sty(&t.header).fg.unwrap_or(Color::Reset);
    let block: Vec<Vec<Line>> = rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let base = if i == *cursor { sty(&t.dialog_input) } else { dstyle(&t) };
            match row {
                Row::Ask { off: None } => vec![Line::from(Span::styled(fit(&format!(" ? {}", t!("find.ask_row", "query" => question.trim())), width), base.patch(hit_style).bg(base.bg.unwrap_or(Color::Reset))))],
                Row::Ask { off: Some(off) } => vec![Line::from(Span::styled(fit(&format!(" ? {}{}", off.text(), step_hint(off)), width), if i == *cursor { base } else { dim }))],
                Row::Head { group, shown, total } => {
                    let label = group.label().to_uppercase();
                    let count = if *total > 0 { t!("find.of", "shown" => shown, "total" => total) } else { String::new() };
                    let gap = width.saturating_sub(label.width() + count.width() + 1);
                    vec![Line::from(Span::styled(format!("{label}{}{count} ", " ".repeat(gap)), dstyle(&t).fg(header_fg).add_modifier(Modifier::BOLD)))]
                }
                Row::More { n, .. } => vec![Line::from(Span::styled(fit(&format!("   {}", t!("find.more", "n" => n)), width), if i == *cursor { base } else { dim }))],
                Row::Off { off, .. } => vec![Line::from(Span::styled(fit(&format!("   {}{}", off.text(), step_hint(off)), width), if i == *cursor { base } else { dim }))],
                Row::Hit { hit: h, .. } => {
                    let p = h.path.to_string_lossy();
                    let (parent, name) = p.rsplit_once(std::path::MAIN_SEPARATOR).unwrap_or(("", &p));
                    let name = if h.is_dir { format!("{name}{}", std::path::MAIN_SEPARATOR) } else { name.to_string() };
                    let pw = width.saturating_sub(name.width() + 2);
                    let file = Line::from(vec![Span::styled(format!(" {name} "), base.patch(hit_style).bg(base.bg.unwrap_or(Color::Reset))), Span::styled(fit(&fit_left(parent, pw), pw), base)]);
                    let Some(snippet) = h.snippet.as_deref() else { return vec![file] };
                    // The words found stand out; a passage close in meaning is in italics.
                    let mut passage = vec![Span::styled("   ", dim)];
                    let rest = if h.similar.is_some() { dim.add_modifier(Modifier::ITALIC) } else { dim };
                    for (n, part) in fit(snippet, width.saturating_sub(3)).split([coxswain_core::store::MARK.0, coxswain_core::store::MARK.1]).enumerate() {
                        passage.push(Span::styled(part.to_string(), if n % 2 == 1 { dstyle(&t).patch(hit_style) } else { rest }));
                    }
                    vec![file, Line::from(passage)]
                }
            }
        })
        .collect();
    // The cursor's row in sight.
    let height = list.height as usize;
    let Some(Dialog::Search { cursor, offset, .. }) = &mut app.dialog else { return };
    *offset = (*offset).min(*cursor);
    while *offset < *cursor && block[*offset..=*cursor].iter().map(Vec::len).sum::<usize>() > height {
        *offset += 1;
    }
    let lines: Vec<Line> = block.into_iter().skip(*offset).flatten().take(height).collect();
    f.render_widget(Paragraph::new(lines), list);
}

/// Find file's name syntax and prefixes (F1 in Find, and in Help).
fn syntax_lines() -> Vec<Line<'static>> {
    let mut v = vec![Line::from(t!("help.syntax"))];
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
    v.push(Line::from(t!("find.help_prefixes")));
    v
}

/// Ask in Find file: each question, its answer as it comes, and its numbered sources; the
/// newest at the bottom, in sight. `off`: why Ask cannot be asked yet.
#[allow(clippy::too_many_arguments)]
fn ask(f: &mut Frame, chat: &[crate::Turn], asking: bool, off: Option<&coxswain_core::find::Off>, search: &coxswain_core::config::SearchConfig, t: &config::Theme, cursor: usize, list: Rect) {
    let dim = dstyle(t).add_modifier(Modifier::DIM);
    let hit = sty(&t.search_hit);
    let mut lines: Vec<Line> = vec![];
    // Not set up, or a chat model that cannot answer: what is missing and the step, in place of the hint.
    match off {
        Some(off @ coxswain_core::find::Off::AskModel(_)) => lines.push(Line::from(Span::styled(off.text(), dstyle(t).fg(Color::Red)))),
        Some(off) => lines.push(Line::from(Span::styled(format!("{} · coxswain --setup-search", off.text()), dim))),
        None if chat.is_empty() => lines.push(Line::from(Span::styled(t!("dialogs.ask_hint", "model" => search.ask_model.as_str()), dim))),
        None => {}
    }
    for (i, turn) in chat.iter().enumerate() {
        lines.push(Line::from(Span::styled(format!("› {}", turn.question), dstyle(t).patch(hit))));
        for text in turn.answer.lines() {
            lines.push(Line::from(text.to_string()));
        }
        // The sources are found; the model has not said a word yet (it may be loading).
        if asking && i + 1 == chat.len() && turn.answer.is_empty() && !turn.sources.is_empty() {
            lines.push(Line::from(Span::styled(t!("dialogs.ask_waiting", "model" => search.ask_model.as_str()), dim)));
        }
        if let Some(e) = &turn.error {
            lines.push(Line::from(Span::styled(e.clone(), dstyle(t).fg(Color::Red))));
        }
        for (n, src) in turn.sources.iter().enumerate() {
            let last = i + 1 == chat.len();
            let style = if last && n == cursor { sty(&t.dialog_input) } else { dim };
            lines.push(Line::from(Span::styled(format!("  [{}] {}", n + 1, src.display()), style)));
        }
        lines.push(Line::from(""));
    }
    // Wrapped, the lines take more rows than there are lines: scroll so the end shows.
    // ponytail: counts characters and one row more per wrapped line, not the real word wraps.
    let width = list.width.max(1) as usize;
    let rows = lines.iter().map(|l| l.width().div_ceil(width).max(1) + usize::from(l.width() > width)).sum::<usize>() as u16;
    let para = Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: false });
    f.render_widget(para.scroll((rows.saturating_sub(list.height), 0)), list);
}

/// Group blocks (a bold heading, then `label  keys` rows, a blank line between groups), laid
/// out in two columns when both fit in `width`, else in one.
fn key_columns(groups: &[(String, Vec<(String, String)>)], width: usize) -> Vec<Line<'static>> {
    let lw = groups.iter().flat_map(|(_, r)| r).map(|(l, _)| l.width()).max().unwrap_or(0);
    let block = |(head, rows): &(String, Vec<(String, String)>)| {
        let mut b = vec![(head.clone(), true)];
        b.extend(rows.iter().map(|(l, k)| (format!("  {l}{} {k}", " ".repeat(lw - l.width())), false)));
        b
    };
    let blocks: Vec<Vec<(String, bool)>> = groups.iter().map(block).collect();
    let join = |bs: &[Vec<(String, bool)>]| bs.join(&(String::new(), false));
    let cw = blocks.iter().flatten().map(|(t, _)| t.width()).max().unwrap_or(0);
    let line = |(t, head): &(String, bool)| if *head { Line::from(Span::raw(t.clone()).bold()) } else { Line::from(t.clone()) };
    if blocks.len() < 2 || width < 2 * cw + 3 {
        return join(&blocks).iter().map(line).collect();
    }
    // Split where the taller column is shortest.
    let rows = |bs: &[Vec<(String, bool)>]| join(bs).len();
    let k = (1..blocks.len()).min_by_key(|&k| rows(&blocks[..k]).max(rows(&blocks[k..]))).unwrap_or(1);
    let (left, right) = (join(&blocks[..k]), join(&blocks[k..]));
    let blank = (String::new(), false);
    (0..left.len().max(right.len()))
        .map(|i| {
            let (l, r) = (left.get(i).unwrap_or(&blank), right.get(i).unwrap_or(&blank));
            let mut spans = line(l).spans;
            spans.push(Span::raw(" ".repeat(cw + 3 - l.0.width())));
            spans.extend(line(r).spans);
            Line::from(spans)
        })
        .collect()
}

pub(crate) fn dstyle(t: &config::Theme) -> Style {
    sty(&t.dialog)
}

/// The help text, its keys under the group headings, in two columns when `width` allows.
fn help_text(app: &App, width: usize) -> Vec<Line<'static>> {
    let mut v = vec![
        Line::from(t!("help.tagline", "version" => coxswain_core::update::VERSION)).bold(),
        Line::from(""),
        Line::from(t!("help.keys")).bold(),
        Line::from(""),
    ];
    let groups: Vec<(String, Vec<(String, String)>)> = config::Group::ALL
        .iter()
        .map(|g| {
            let rows = g
                .actions()
                .filter(|a| !a.gui_only())
                .map(|a| (a.label(), app.cfg.keys.get(&a).map(|k| k.join(", ")).unwrap_or_default()))
                .collect();
            (g.label(), rows)
        })
        .filter(|(_, rows): &(String, Vec<_>)| !rows.is_empty())
        .collect();
    v.extend(key_columns(&groups, width));
    v.push(Line::from(""));
    for k in ["help.also1", "help.also2", "help.also3", "help.bom", "help.history"] {
        v.push(Line::from(t!(k, "key" => app.key_label(Action::History))));
    }
    let key = |a| app.key_label(a).to_string();
    v.push(Line::from(t!("help.branches", "key" => key(Action::Branches), "switch" => key(Action::SwitchBranch), "worktrees" => key(Action::Worktrees))));
    v.push(Line::from(""));
    v.extend(syntax_lines());
    v.push(Line::from(""));
    v.push(Line::from(t!("help.config", "path" => config::Config::path().map(|p| p.display().to_string()).unwrap_or_default())));
    v.push(Line::from(t!("help.dump")));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_keys_take_two_columns_when_wide() {
        let g = |h: &str, n: usize| (h.to_string(), (0..n).map(|i| (format!("act{i}"), "F1".to_string())).collect::<Vec<_>>());
        let groups = [g("A", 3), g("B", 1), g("C", 2)];
        let text = |w| key_columns(&groups, w).iter().map(|l| l.to_string()).collect::<Vec<_>>();
        let narrow = text(20);
        assert_eq!(narrow.len(), 4 + 2 + 3 + 2, "one column: headings, rows and two blank lines");
        let wide = text(80);
        assert_eq!(wide.len(), 6, "A beside B and C");
        assert!(wide[0].starts_with('A') && wide[0].trim_end().ends_with('B'), "{wide:?}");
        assert!(wide.iter().all(|l| l.width() <= 80));
    }
}
