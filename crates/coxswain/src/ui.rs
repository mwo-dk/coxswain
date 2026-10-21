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
use unicode_width::UnicodeWidthChar;

pub(crate) fn sty(s: &config::Style) -> Style {
    // `#rrggbb`, what the themes write, read directly: `Color::from_str` tries the colour
    // names first, which costs more than the rest of a row's drawing.
    let color = |c: &str| match c.strip_prefix('#').filter(|h| h.len() == 6 && h.bytes().all(|b| b.is_ascii_hexdigit())).and_then(|h| u32::from_str_radix(h, 16).ok()) {
        Some(n) => Some(Color::Rgb((n >> 16) as u8, (n >> 8) as u8, n as u8)),
        _ => (!c.is_empty()).then(|| Color::from_str(c).ok()).flatten(),
    };
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

/// `s` with each control character (a newline, a tab, an escape in a file's name) as `?`, as
/// `ls` shows them: printed, they would move the cursor or talk to the terminal.
pub(crate) fn printable(s: &str) -> std::borrow::Cow<'_, str> {
    if s.chars().any(char::is_control) {
        s.chars().map(|c| if c.is_control() { '?' } else { c }).collect::<String>().into()
    } else {
        s.into()
    }
}

/// The columns a text takes in a terminal: the sum of its letters', each in a cell of its own,
/// as the terminal and ratatui place them. `UnicodeWidthStr` counts Arabic lam-alef as one
/// column (a ligature), which pushed a Persian "بالاتر" one column past its border.
// ponytail: letters, not grapheme clusters; an emoji with a variation selector may differ.
pub(crate) trait Cols {
    fn width(&self) -> usize;
}

impl Cols for str {
    fn width(&self) -> usize {
        self.chars().map(|c| c.width().unwrap_or(0)).sum()
    }
}

/// Cut to `w` columns, marking the cut with `…`; pad to exactly `w`.
pub(crate) fn fit(s: &str, w: usize) -> String {
    let s = &*printable(s);
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
    let s = &*printable(s);
    let gap = w.saturating_sub(s.width());
    format!("{}{s}{}", " ".repeat(gap / 2), " ".repeat(gap - gap / 2))
}

/// `s` against the right edge of `w` columns, cut to fit.
fn right(s: &str, w: usize) -> String {
    fit(&format!("{}{s}", " ".repeat(w.saturating_sub(s.width()))), w)
}

/// `s` for the middle of a border `w` wide whose ends hold `left` and `right` columns of titles:
/// cut so that, centred, it stays clear of both (the git line on ZFS keeps its branch); none
/// when too little is left to say anything.
fn centred_between(s: &str, w: usize, left: usize, right: usize) -> Option<String> {
    // The corners, both ends' titles as wide as the wider one, and the spaces around `s`.
    let room = w.saturating_sub(2 + 2 * left.max(right) + 2);
    if s.width() <= room {
        Some(s.to_string())
    } else if room >= 12 {
        Some(fit(s, room).trim_end().to_string())
    } else {
        None
    }
}

/// Keep the tail of a path, which is the informative part.
pub(crate) fn fit_left(s: &str, w: usize) -> String {
    let s = &*printable(s);
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

pub(crate) fn size(n: u64) -> String {
    coxswain_core::i18n::digits(size_latin(n))
}

fn size_latin(n: u64) -> String {
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

pub(crate) fn date(secs: u64) -> String {
    coxswain_core::i18n::digits(Local.timestamp_opt(secs as i64, 0).single().map(|t| t.format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default())
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
        Some(Dialog::Provenance(v)) => v.draw(f, &theme),
        Some(Dialog::GitConfig(v)) => v.draw(f, &theme),
        Some(Dialog::View { v, .. }) => v.draw(f, &theme),
        Some(Dialog::Edit { e, .. }) => e.draw(f, &theme),
        Some(Dialog::Chess(v)) => v.draw(f, &theme),
        Some(Dialog::Go(v)) => v.draw(f, &theme),
        Some(Dialog::Space(v)) => v.draw(f, &theme),
        Some(Dialog::Settings(_)) => crate::settings::draw(f, app),
        Some(Dialog::Guide(_)) => crate::guide::draw(f, app),
        Some(_) => dialog(f, app),
        None => {}
    }
    // Whatever was drawn without `fit` (a dialog's text, an error): no control character
    // reaches the terminal, where it would move the cursor or start an escape sequence. Drawn
    // as taking no room, it rides along in the cell before it; taken out, the row stays even.
    for cell in f.buffer_mut().content.iter_mut() {
        if cell.symbol().chars().any(char::is_control) {
            let clean: String = cell.symbol().chars().filter(|c| !c.is_control()).collect();
            cell.set_symbol(if clean.is_empty() { " " } else { &clean });
        }
    }
}

/// What a panel's folder is, as its title's badge says: read from the disk once per folder.
pub enum Place {
    Archive,
    History(coxswain_core::history::At),
    /// A dataset's list of snapshots.
    Snapshots,
    Plain,
}

impl Place {
    fn of(dir: &std::path::Path) -> Place {
        if coxswain_core::archive::split(dir).is_some() {
            Place::Archive
        } else if let Some(at) = coxswain_core::history::split(dir) {
            Place::History(at)
        } else if coxswain_core::zfs::split(dir).is_some() {
            Place::Snapshots
        } else {
            Place::Plain
        }
    }
}

fn panel(f: &mut Frame, app: &mut App, side: usize, area: Rect) {
    // What drawing needs from the disk, read when the folder or the symlink under the cursor
    // is new, not at every frame.
    let p = &mut app.panels[side];
    if p.place.as_ref().is_none_or(|(d, _)| *d != p.dir) {
        p.place = Some((p.dir.clone(), Place::of(&p.dir)));
        p.models = Some((p.dir.clone(), coxswain_core::local_models::names(&p.dir)));
    }
    if let Some(e) = p.current().filter(|e| e.is_symlink)
        && p.link.as_ref().is_none_or(|(at, _)| *at != e.path)
    {
        p.link = Some((e.path.clone(), std::fs::read_link(&e.path).ok()));
    }
    let t = &app.theme;
    let active = side == app.active && app.dialog.is_none();
    let (base, border) = (sty(&t.panel), sty(&t.border).bg(sty(&t.panel).bg.unwrap_or(Color::Reset)));
    let p = &app.panels[side];

    let title_style = if side == app.active { sty(&t.cursor) } else { border };
    // Inside an archive the title says so, so a copy out is not taken for one between folders.
    // So is a history, with the commit looked into.
    let place = p.place.as_ref().map_or(&Place::Plain, |(_, pl)| pl);
    let dir = match place {
        // A disk image (CHM, CBR) says it is read-only instead.
        Place::Archive => {
            let archive = coxswain_core::archive::split(&p.dir).map(|(a, _)| a);
            let ro = archive.as_deref().is_some_and(coxswain_core::archive::read_only);
            let badge = if ro { t!("dialogs.readonly") } else { t!("archive.badge") };
            // A container image says which: its tag.
            match archive.as_deref().filter(|a| coxswain_core::archive::is_image(a)).and_then(coxswain_core::container_image::title) {
                Some(tag) => format!("{} [{tag} · {badge}]", p.dir.to_string_lossy()),
                None => format!("{} [{badge}]", p.dir.to_string_lossy()),
            }
        }
        Place::History(at) => match (&at.commit, at.view) {
            (Some(c), _) => format!("{} [{}]", p.dir.to_string_lossy(), t!("history.badge_at", "commit" => &c[..c.len().min(7)])),
            (None, coxswain_core::history::View::History) => format!("{} [{}]", p.dir.to_string_lossy(), t!("history.badge")),
            (None, coxswain_core::history::View::Branches) => format!("{} [{}]", p.dir.to_string_lossy(), t!("branches.badge")),
            (None, coxswain_core::history::View::Worktrees) => format!("{} [{}]", p.dir.to_string_lossy(), t!("worktrees.badge")),
        },
        // Among a dataset's snapshots: which, of which dataset.
        _ => match (coxswain_core::zfs::in_snapshot(&p.dir), matches!(place, Place::Snapshots).then_some(()), &p.package) {
            (Some(s), ..) => {
                let ds = p.zfs.as_ref().map_or(String::new(), |f| f.dataset.clone());
                format!("{} [{}]", p.dir.to_string_lossy(), t!("zfs.badge_at", "name" => s.name, "dataset" => ds))
            }
            (_, Some(_), _) => format!("{} [{}]", p.dir.to_string_lossy(), t!("zfs.badge_list", "dataset" => p.zfs.as_ref().map_or("", |f| f.dataset.as_str()))),
            (.., Some(pkg)) => format!("{} [{}]", p.dir.to_string_lossy(), t!("pkg.badge", "name" => pkg)),
            _ => p.dir.to_string_lossy().into_owned(),
        },
    };
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(border)
        .style(base)
        .title(Line::from(Span::styled(format!(" {} ", fit_left(&dir, area.width.saturating_sub(6) as usize)), title_style)).centered());
    let mut git_w = 0;
    if let Some(g) = &p.git {
        let prompt = g.prompt(&app.glyphs);
        git_w = prompt.width() + 2;
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
    // On ZFS: the dataset, its compression ratio and its space, between the git line and the sort.
    if let Some(z) = p.zfs.as_ref().and_then(|z| centred_between(&crate::App::zfs_line(z), area.width as usize, git_w, sort.width() + 2)) {
        block = block.title_bottom(Line::from(Span::styled(format!(" {z} "), border)).centered());
    }
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
        // In a model store: the model it is, before the hashed name.
        let name = match p.models.as_ref().and_then(|(_, m)| m.as_ref()).and_then(|m| m.get(&e.name)) {
            Some(m) => format!("{} {m} [{}]", coxswain_core::icons::entry(&e.name, e.is_dir, e.is_symlink, plain).glyph, e.name),
            None => name,
        };
        let name = &name;
        let sz = if let Some(n) = p.sizes.get(&e.path) {
            size(*n)
        } else if e.is_parent() {
            t!("tui.updir")
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
        // A snapshot in the list: its own space, and the data it refers to.
        if e.referenced > 0 {
            right = format!(" {}", t!("zfs.used_referenced", "used" => size(e.size), "referenced" => size(e.referenced)));
        }
        // The last commit that changed it, when git has said.
        match p.last.as_ref().and_then(|l| l.get(&e.name)) {
            Some(Some(c)) => right = format!(" {} {} {}{right}", c.hash, date(c.time), c.author),
            Some(None) => right = format!(" {}{right}", t!("history.older")),
            None => {}
        }
        // A repository's folder, or `..` in one: its branch, upstream, remote and last fetch.
        if let Some((at, Some(line))) = &p.facts
            && p.facts_dir().as_ref() == Some(at)
        {
            right = format!(" {}{right}", fit(line, w.saturating_sub(e.name.width() + 2).min(line.width())));
        }
        if e.online {
            right = format!(" {}{right}", t!("tui.online", "key" => app.key_label(coxswain_core::config::Action::View)));
        }
        let mut name = e.name.clone();
        if let Some((_, Some(target))) = p.link.as_ref().filter(|(at, _)| e.is_symlink && *at == e.path) {
            name = format!("{name} -> {}", target.display());
        }
        // A narrow panel keeps room for the name: the facts on the right give way first, from
        // their start, so the size at their end stays.
        let keep = name.width().min(w / 2);
        let right = if right.width() + keep > w { fit_left(&right, w.saturating_sub(keep)) } else { right };
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
        let mut line = Line::from(Span::styled(fit_left(&full, area.width as usize), st));
        // While the line is empty: a hint that fits what is under the cursor, dimmed.
        if let (Some((_, hint)), true, None) = (&app.hint, app.cmdline.is_empty(), &app.dialog) {
            let room = (area.width as usize).saturating_sub(prompt.width());
            line.push_span(Span::styled(fit(hint, room), st.add_modifier(Modifier::DIM)));
        }
        (line, Some(x.min(area.right().saturating_sub(1))))
    };
    f.render_widget(Paragraph::new(line).style(st), area);
    if let (Some(x), None) = (cursor, &app.dialog) {
        f.set_cursor_position(Position::new(x, area.y));
    }
}

fn keybar(f: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;
    // F1 to F10 as in NC, then F11 and F12 when they run something (F12: Settings).
    let fkey = |n| app.keymap.get(&Key::new(KeyCode::F(n), false, false, false));
    let keys: Vec<u8> = (1..=12u8).filter(|&n| n <= 10 || fkey(n).is_some()).collect();
    // The keymap, so the bar names the action the key runs when two share it.
    // Norton Commander's short words where the plain ones do not fit: RenMov, Mkdir, PullDn.
    let labels: Vec<String> = keys
        .iter()
        .map(|&n| {
            fkey(n).map_or(String::new(), |a| match a {
                Action::Move | Action::NewFolder | Action::Menu => t!(&format!("tui.keybar.{}", a.name())),
                a => a.label(),
            })
        })
        .collect();
    let nums: Vec<String> = keys.iter().map(|n| n.to_string()).collect();
    let widths = keybar_widths(&nums.iter().map(|n| n.len()).collect::<Vec<_>>(), &labels.iter().map(|l| l.width()).collect::<Vec<_>>(), area.width as usize);
    let mut spans = vec![];
    for ((num, label), lw) in nums.into_iter().zip(&labels).zip(widths) {
        spans.push(Span::styled(num, sty(&t.keybar_num)));
        spans.push(Span::styled(fit(label, lw), sty(&t.keybar_label)));
    }
    f.render_widget(Paragraph::new(Line::from(spans)).style(sty(&t.keybar_num)), area);
}

/// The columns each F-key label gets: even slots, as in NC, when every label fits in its own
/// (the last slot takes what is left over, so "12Settings" fits in 80 columns). When one does
/// not, as in most languages at 80 columns, short labels lend their spare room to long ones,
/// so only the longest are cut.
fn keybar_widths(nums: &[usize], labels: &[usize], width: usize) -> Vec<usize> {
    let k = nums.len().max(1);
    let slot = width / k;
    let even: Vec<usize> = (0..nums.len()).map(|i| if i + 1 == k { width - slot * i } else { slot }.saturating_sub(nums[i])).collect();
    if labels.iter().zip(&even).all(|(l, e)| l <= e) {
        return even;
    }
    let avail = width.saturating_sub(nums.iter().sum());
    // The largest cap that fits, then what is left makes whole the labels it cut least.
    let mut cap = labels.iter().copied().max().unwrap_or(0);
    while cap > 0 && labels.iter().map(|&l| l.min(cap)).sum::<usize>() > avail {
        cap -= 1;
    }
    let mut out: Vec<usize> = labels.iter().map(|&l| l.min(cap)).collect();
    let mut spare = avail - out.iter().sum::<usize>();
    let mut cut: Vec<usize> = (0..out.len()).filter(|&i| out[i] < labels[i]).collect();
    cut.sort_by_key(|&i| labels[i] - out[i]);
    for i in cut {
        let more = (labels[i] - out[i]).min(spare);
        out[i] += more;
        spare -= more;
    }
    // Any room still over pads the labels evenly, from the left.
    let n = out.len();
    for i in 0..spare {
        out[i % n] += 1;
    }
    out
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h }
}

/// A box of `w` columns, as tall as `p` wraps in it plus `extra` rows (the frame, a gap, the
/// keys), at most the screen: a translated text is never cut off.
fn text_box(full: Rect, w: u16, p: &Paragraph, extra: u16) -> Rect {
    let w = w.min(full.width);
    let h = (p.line_count(w.saturating_sub(2)) as u16).saturating_add(extra);
    centered(full, w, h)
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
            let inner = frame(f, app, centered(full, (label.width() as u16 + 4).clamp(70, 110), 6), title);
            let [a, b, _, c] = Layout::vertical([Constraint::Length(1); 4]).areas(inner.inner(ratatui::layout::Margin::new(1, 0)));
            f.render_widget(Paragraph::new(label.as_str()), a);
            // A password shows as stars.
            let shown = if matches!(prompt, crate::Prompt::Password(..) | crate::Prompt::Unlock(..) | crate::Prompt::Peek(..) | crate::Prompt::PackPassword(..) | crate::Prompt::PackConfirm(..)) { "*".repeat(value.chars().count()) } else { value.clone() };
            input_line(f, app, b, &shown);
            f.render_widget(Paragraph::new(keys(&prompt.verb())).centered(), c);
        }
        Dialog::Confirm { title, text, .. } | Dialog::Switch { title, text, .. } => {
            let verb = match app.dialog.as_ref() {
                Some(Dialog::Confirm { forever: false, .. }) => t!("app.move_to_trash"),
                Some(Dialog::Confirm { .. }) => t!("common.delete"),
                _ => t!("verb.switch"),
            };
            let p = Paragraph::new(text.as_str()).centered().wrap(Wrap { trim: true });
            let inner = frame(f, app, text_box(full, 60, &p, 4), title);
            let [a, _, b] = Layout::vertical([Constraint::Min(0), Constraint::Length(1), Constraint::Length(1)]).areas(inner);
            f.render_widget(p, a);
            f.render_widget(Paragraph::new(keys(&verb)).centered(), b);
        }
        d @ (Dialog::Searchable { title, text, .. } | Dialog::Media { title, text, .. }) => {
            let p = Paragraph::new(text.as_str()).wrap(Wrap { trim: true });
            let inner = frame(f, app, text_box(full, 64, &p, 4), title);
            let [a, _, b] = Layout::vertical([Constraint::Min(0), Constraint::Length(1), Constraint::Length(1)]).areas(inner);
            f.render_widget(p, a);
            let verb = if matches!(d, Dialog::Media { .. }) { t!("media.go") } else { t!("searchable.go") };
            f.render_widget(Paragraph::new(keys(&verb)).centered(), b);
        }
        Dialog::Message { title, text } => {
            let p = Paragraph::new(text.as_str()).wrap(Wrap { trim: false });
            let inner = frame(f, app, text_box(full, 76, &p, 4), title);
            let [body, key] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(inner);
            f.render_widget(p, body);
            f.render_widget(Paragraph::new(t!("dialog.keys_one", "verb" => t!("common.close"))).centered(), key);
        }
        Dialog::Help { scroll } => {
            let area = centered(full, 120, full.height.saturating_sub(4));
            let inner = frame(f, app, area, &t!("help.title")).inner(ratatui::layout::Margin::new(1, 0));
            let [body, key] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(inner);
            let text = help_text(app, body.width as usize);
            f.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }).scroll((*scroll, 0)), body);
            f.render_widget(Paragraph::new(fit(&t!("help.features"), key.width as usize)).bold().centered(), key);
        }
        Dialog::Features { filter, list, cursor, said, .. } => {
            let area = centered(full, 120, full.height.saturating_sub(4));
            let inner = frame(f, app, area, &t!("features.title")).inner(ratatui::layout::Margin::new(1, 0));
            let [field, body, key] = Layout::vertical([Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)]).areas(inner);
            let (lines, at) = feature_lines(list, *cursor, filter.is_empty(), body.width as usize, &t);
            let lines = if list.is_empty() { vec![Line::from(t!("features.none"))] } else { lines };
            // The feature under the cursor in the upper third.
            let skip = at.saturating_sub(body.height as usize / 3);
            f.render_widget(Paragraph::new(lines).scroll((skip as u16, 0)), body);
            let foot = said.clone().unwrap_or_else(|| t!("features.keys_tui"));
            f.render_widget(Paragraph::new(fit(&foot, key.width as usize)).centered(), key);
            input_line(f, app, field, filter);
        }
        Dialog::Menu { title, filter, items, cursor, direct, .. } => {
            let visible: Vec<&crate::MenuItem> = crate::menu_visible(items, filter).into_iter().map(|i| &items[i]).collect();
            let kw = items.iter().map(|i| i.key.width()).max().unwrap_or(0);
            let lw = items.iter().map(|i| i.label.width()).max().unwrap_or(0).max(items.iter().filter_map(|i| i.group.as_ref()).map(|g| g.width()).max().unwrap_or(0));
            // The rows: a heading where the group changes (the action menu), then the items;
            // `Some(i)` is the i-th visible item.
            let mut lines: Vec<(Option<usize>, &str)> = vec![];
            for (i, it) in visible.iter().enumerate() {
                if let Some(g) = it.group.as_deref().filter(|g| i == 0 || visible[i - 1].group.as_deref() != Some(*g)) {
                    lines.push((None, g));
                }
                lines.push((Some(i), ""));
            }
            let h = (lines.len() as u16 + 2 + !direct as u16).min(full.height.saturating_sub(2));
            let inner = frame(f, app, centered(full, (kw + lw + 6) as u16, h), title);
            let mut y = inner.y;
            if !direct {
                input_line(f, app, Rect { y, height: 1, ..inner }, filter);
                y += 1;
            }
            let rows = inner.bottom().saturating_sub(y) as usize;
            let at = lines.iter().position(|l| l.0 == Some(*cursor)).unwrap_or(0);
            let skip = at.saturating_sub(rows.saturating_sub(1));
            for (item, heading) in lines.into_iter().skip(skip).take(rows) {
                let (line, s) = match item {
                    Some(i) => (format!(" {} {}", fit(&visible[i].label, lw), fit(&visible[i].key, kw)), if i == *cursor { sty(&t.dialog_input) } else { dstyle }),
                    None => (format!(" {heading}"), dstyle.add_modifier(Modifier::BOLD | Modifier::DIM)),
                };
                f.render_widget(Paragraph::new(fit(&line, inner.width as usize)).style(s), Rect { y, height: 1, ..inner });
                y += 1;
            }
        }
        Dialog::Search { .. } => search(f, app, full),
        Dialog::Bom(_) | Dialog::Provenance(_) | Dialog::GitConfig(_) | Dialog::View { .. } | Dialog::Edit { .. } | Dialog::Chess(_) | Dialog::Go(_) | Dialog::Space(_) | Dialog::Settings(_) | Dialog::Guide(_) => {}
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
    let (query, chip, here, show, cursor) = (query.clone(), *chip, *here, *show, *cursor);
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
    // The scope as a choice of two, the one in force inverted: [everywhere | in rocket].
    let scope_key = app.cfg.handy_key(Action::Search).unwrap_or("").to_string();
    let short = fit_left(&folder, 30);
    let (all, inside) = (format!(" {} ", t!("dialogs.scope_everywhere").to_lowercase()), format!(" {} ", t!("dialogs.scope_in", "folder" => &short).to_lowercase()));
    let on = dstyle(&t).patch(hit_style).add_modifier(Modifier::REVERSED);
    let off = dstyle(&t).patch(hit_style);
    let scope = Line::from(vec![
        Span::styled(" [", off),
        Span::styled(all, if here { off } else { on }),
        Span::styled("|", off),
        Span::styled(inside, if here { on } else { off }),
        Span::styled("]", off),
    ]);
    f.render_widget(Paragraph::new(prompt.as_str()), q);
    let w = (q.width as usize).saturating_sub(prompt.width() + scope.width());
    let qa = Rect { x: q.x + prompt.width() as u16, width: w as u16, ..q };
    let shown = fit_left(&query, w.saturating_sub(1));
    if query.is_empty() && !answer {
        // The empty field names the scope and the key that changes it.
        let ph = if here { t!("find.placeholder_here", "folder" => &short, "key" => &scope_key) } else { t!("find.placeholder_everywhere", "folder" => &short, "key" => &scope_key) };
        f.render_widget(Paragraph::new(fit(&ph, w)).style(sty(&t.dialog_input).add_modifier(Modifier::DIM)), qa);
    } else {
        f.render_widget(Paragraph::new(fit(&shown, w)).style(sty(&t.dialog_input)), qa);
    }
    f.render_widget(Paragraph::new(scope), Rect { x: qa.x + w as u16, width: q.width.saturating_sub(prompt.width() as u16 + w as u16), ..q });
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
    let keys_text = if answer { t!("find.keys_answer_tui") } else { t!("find.keys_tui", "scope" => scope_key) };
    f.render_widget(Paragraph::new(fit(&keys_text, keys.width as usize)).centered(), keys);

    if show == crate::Show::Syntax {
        let mut lines = syntax_lines();
        lines.extend([Line::from(""), Line::from(t!("find.f1_features")).bold()]);
        return f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), list);
    }
    if answer {
        let off = app.ask_state().err();
        return ask(f, &app.chat, app.ask_rx.is_some(), off.as_ref(), &app.cfg.search, &t, cursor, list);
    }
    let width = list.width as usize;
    if question.trim().is_empty() {
        let mut lines = vec![Line::from(t!("find.empty"))];
        if app.find_hint && !here {
            lines.extend([Line::from(""), Line::from(Span::styled(t!("hint.find_scope", "key" => &scope_key, "folder" => &folder), dim))]);
        }
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
    // The rows are rebuilt every frame and can shrink under the cursor (a tip dismissed, the
    // slow-model row going): the cursor goes back onto a row that is there.
    let cursor = find::step(&rows, cursor, 0);

    // Each row as lines; a hit with a passage takes two.
    let step_hint = |off: &find::Off| {
        let mut s = format!(" · Enter: {}", off.step());
        if off.dismiss_id().is_some() {
            s += &format!(" · {}", t!("find.dismiss"));
        }
        s
    };
    let header_fg = sty(&t.header).fg.unwrap_or(Color::Reset);
    // The cursor's row in sight; only the rows that fit are made into lines.
    let height = list.height as usize;
    let Some(Dialog::Search { cursor: at, offset, .. }) = &mut app.dialog else { return };
    *at = cursor;
    let shown_rows = in_sight(&rows, offset, cursor, height);
    let lines: Vec<Line> = rows[shown_rows.clone()]
        .iter()
        .zip(shown_rows)
        .flat_map(|(row, i)| {
            let base = if i == cursor { sty(&t.dialog_input) } else { dstyle(&t) };
            match row {
                Row::Ask { off: None } => vec![Line::from(Span::styled(fit(&format!(" ? {}", t!("find.ask_row", "query" => question.trim())), width), base.patch(hit_style).bg(base.bg.unwrap_or(Color::Reset))))],
                Row::Slow { poor } => {
                    let step = poor.button.clone().unwrap_or_else(|| t!("find.step_settings"));
                    vec![Line::from(Span::styled(fit(&format!("   {} · Enter: {step}", poor.short), width), if i == cursor { base } else { dim }))]
                }
                Row::Ask { off: Some(off) } => vec![Line::from(Span::styled(fit(&format!(" ? {}{}", off.text(), step_hint(off)), width), if i == cursor { base } else { dim }))],
                Row::Head { group, shown, total } => {
                    let label = coxswain_core::i18n::caps(&group.label());
                    let count = if *total > 0 { t!("find.of", "shown" => shown, "total" => total) } else { String::new() };
                    let gap = width.saturating_sub(label.width() + count.width() + 1);
                    vec![Line::from(Span::styled(format!("{label}{}{count} ", " ".repeat(gap)), dstyle(&t).fg(header_fg).add_modifier(Modifier::BOLD)))]
                }
                Row::More { n, .. } => vec![Line::from(Span::styled(fit(&format!("   {}", t!("find.more", "n" => n)), width), if i == cursor { base } else { dim }))],
                Row::Off { off, .. } => vec![Line::from(Span::styled(fit(&format!("   {}{}", off.text(), step_hint(off)), width), if i == cursor { base } else { dim }))],
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
        .take(height)
        .collect();
    f.render_widget(Paragraph::new(lines), list);
}

/// How many lines a row of Find takes: a hit with a passage two, every other row one.
fn row_lines(row: &coxswain_core::find::Row) -> usize {
    match row {
        coxswain_core::find::Row::Hit { hit, .. } if hit.snippet.is_some() => 2,
        _ => 1,
    }
}

/// Moves `offset` so the cursor's row is in sight in `height` lines, and gives the rows from
/// there that fit (the last one may be cut).
fn in_sight(rows: &[coxswain_core::find::Row], offset: &mut usize, cursor: usize, height: usize) -> std::ops::Range<usize> {
    *offset = (*offset).min(cursor);
    let mut used: usize = rows[*offset..=cursor].iter().map(row_lines).sum();
    while *offset < cursor && used > height {
        used -= row_lines(&rows[*offset]);
        *offset += 1;
    }
    let (mut end, mut used) = (*offset, 0);
    while end < rows.len() && used < height {
        used += row_lines(&rows[end]);
        end += 1;
    }
    *offset..end
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
        None if chat.is_empty() => lines.push(Line::from(Span::styled(t!("dialogs.ask_hint", "model" => coxswain_core::chat::shown(&search.ask_model)), dim))),
        None => {}
    }
    for (i, turn) in chat.iter().enumerate() {
        lines.push(Line::from(Span::styled(format!("› {}", turn.question), dstyle(t).patch(hit))));
        for text in turn.answer.lines() {
            lines.push(Line::from(text.to_string()));
        }
        // The sources are found; the model has not said a word yet (it may be loading).
        if asking && i + 1 == chat.len() && turn.answer.is_empty() && !turn.sources.is_empty() {
            lines.push(Line::from(Span::styled(t!("dialogs.ask_waiting", "model" => coxswain_core::chat::shown(&search.ask_model)), dim)));
        }
        if let Some(e) = &turn.error {
            lines.push(Line::from(Span::styled(e.clone(), dstyle(t).fg(Color::Red))));
        }
        if !turn.sources.is_empty() {
            lines.push(Line::from(Span::styled(format!("  {}", turn.read), dim)));
        }
        for (n, src) in turn.sources.iter().enumerate() {
            let last = i + 1 == chat.len();
            let style = if last && n == cursor { sty(&t.dialog_input) } else { dim };
            let similar = match turn.similar.get(n) {
                Some(&k) if k > 0 => format!("  {}", coxswain_core::tn!("ask.similar_files", k)),
                _ => String::new(),
            };
            lines.push(Line::from(Span::styled(format!("  [{}] {}{similar}", n + 1, src.display()), style)));
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

/// A dialog's keys, the line both apps show: "Enter Copy · Esc Cancel".
pub(crate) fn keys(verb: &str) -> String {
    t!("dialog.keys", "verb" => verb, "cancel" => t!("common.cancel"))
}

pub(crate) fn dstyle(t: &config::Theme) -> Style {
    sty(&t.dialog)
}

/// `s` in lines of at most `width` columns, broken at spaces, or anywhere in a word that does
/// not fit (Japanese has no spaces).
fn wrap(s: &str, width: usize) -> Vec<String> {
    let width = width.max(8);
    let (mut out, mut line, mut w) = (vec![], String::new(), 0);
    for word in s.split_inclusive(' ') {
        let ww = word.trim_end().width();
        // Japanese and Korean break between any two letters, so a word with wide letters
        // flows on from where the line is rather than starting a line of its own.
        let wide = word.chars().any(|c| c.width() == Some(2));
        if w + ww > width && w > 0 && !wide {
            out.push(std::mem::take(&mut line).trim_end().to_string());
            w = 0;
        }
        for c in word.chars() {
            let cw = c.width().unwrap_or(0);
            if w + cw > width && c != ' ' {
                out.push(std::mem::take(&mut line));
                w = 0;
            }
            line.push(c);
            w += cw;
        }
    }
    if !line.trim().is_empty() {
        out.push(line.trim_end().to_string());
    }
    out
}

/// F1 → Features as lines: an area heading where the area changes (unfiltered), each feature's
/// title with its keys, its line, and its questions with their answers. Also the line of the
/// feature under the cursor.
fn feature_lines(list: &[coxswain_core::features::Entry], cursor: usize, headings: bool, width: usize, t: &config::Theme) -> (Vec<Line<'static>>, usize) {
    let (mut v, mut at) = (vec![], 0);
    for (i, e) in list.iter().enumerate() {
        if headings && (i == 0 || list[i - 1].area != e.area) {
            if i > 0 {
                v.push(Line::from(""));
            }
            v.push(Line::from(Span::styled(e.area.clone(), dstyle(t).add_modifier(Modifier::BOLD | Modifier::DIM))));
        }
        if i == cursor {
            at = v.len();
        }
        let keys: Vec<String> = e.keys.iter().map(|(a, k)| format!("{a} {k}")).collect();
        let head = if keys.is_empty() { format!(" {} ", e.title) } else { format!(" {}   {} ", e.title, keys.join(" · ")) };
        let style = if i == cursor { sty(&t.dialog_input) } else { dstyle(t).add_modifier(Modifier::BOLD) };
        v.push(Line::from(Span::styled(fit(&head, width), style)));
        v.extend(wrap(&e.what, width.saturating_sub(3)).into_iter().map(|l| Line::from(format!("   {l}"))));
        for (q, a) in &e.qa {
            v.extend(wrap(q, width.saturating_sub(3)).into_iter().map(|l| Line::from(format!("   {l}")).bold()));
            v.extend(wrap(a, width.saturating_sub(5)).into_iter().map(|l| Line::from(format!("     {l}"))));
        }
    }
    (v, at)
}

/// The help text, its keys under the group headings, in two columns when `width` allows.
fn help_text(app: &App, width: usize) -> Vec<Line<'static>> {
    // `{view}`, `{menu}` … in the texts: the keys of the user's keymap.
    let keys = |s: String| coxswain_core::features::with_keys(&app.cfg, &s);
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
                .map(|a| (app.action_label(a), app.cfg.keys.get(&a).map(|k| k.join(", ")).unwrap_or_default()))
                .collect();
            (g.label(), rows)
        })
        .filter(|(_, rows): &(String, Vec<_>)| !rows.is_empty())
        .collect();
    v.extend(key_columns(&groups, width));
    v.push(Line::from(""));
    for k in ["help.also1", "help.also2", "help.also3", "help.also4", "help.viewer", "help.bom", "help.provenance", "help.history"] {
        v.push(Line::from(keys(t!(k, "key" => app.key_label(Action::History)))));
    }
    let key = |a| app.key_label(a).to_string();
    v.push(Line::from(keys(t!("help.branches", "key" => key(Action::Branches), "switch" => key(Action::SwitchBranch), "worktrees" => key(Action::Worktrees)))));
    v.push(Line::from(keys(t!("help.git_tool", "key" => key(Action::GitTool)))));
    v.push(Line::from(keys(t!("help.git_config", "key" => key(Action::GitConfig)))));
    v.push(Line::from(keys(t!("help.searchable_pdf", "key" => key(Action::SearchablePdf)))));
    v.push(Line::from(keys(t!("help.chess", "key" => key(Action::Chess), "view" => key(Action::View)))));
    v.push(Line::from(keys(t!("help.space", "key" => key(Action::SpaceView)))));
    v.push(Line::from(""));
    v.extend(syntax_lines());
    v.push(Line::from(""));
    v.push(Line::from(t!("help.config", "path" => config::Config::path().map(|p| p.display().to_string()).unwrap_or_default())));
    v.push(Line::from(t!("help.dump")));
    v.push(Line::from(""));
    v.push(Line::from(t!("help.guide")).bold());
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// On ZFS in a git repository, the dataset line is cut before it covers the branch.
    #[test]
    fn zfs_line_leaves_the_git_branch_and_the_sort_alone() {
        let z = "rpool/home/alice · 2.48x · 3670M used · 12G free";
        assert_eq!(centred_between(z, 120, 13, 3).as_deref(), Some(z));
        let cut = centred_between(z, 60, 13, 3).unwrap();
        assert!(cut.ends_with('…') && cut.width() <= 60 - 2 - 26 - 2, "{cut}");
        assert_eq!(centred_between(z, 60, 0, 3).as_deref(), Some(z), "without git it fits");
        assert_eq!(centred_between(z, 30, 13, 3), None);
    }

    /// The Make PDF searchable question, and the other text dialogs, show their last sentence
    /// however long it wraps, in English, German and Finnish, narrow and wide.
    #[test]
    fn text_dialogs_show_their_last_line() {
        let d = std::env::temp_dir().join(format!("coxswain-test-dialog-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let cfg = coxswain_core::config::Config { check_updates: false, ..Default::default() };
        let index = crate::test_index(&cfg);
        let mut app = App::with_index(cfg, d.clone(), d.clone(), index).unwrap();
        for lang in ["en-GB", "de", "fi"] {
            // From the catalogue, not the process's language, which other tests share.
            let cat = coxswain_core::i18n::catalogue(lang);
            let tr = |k: &str| cat[k].as_str().unwrap().to_string();
            let image = "docker.io/jbarlow83/ocrmypdf-alpine:latest";
            let text = format!(
                "{} {} {}.",
                tr("searchable.confirm_one").replace("{target}", "Jahresbericht 2025 eingescannt.searchable.pdf").replace("{name}", "Jahresbericht 2025 eingescannt.pdf"),
                tr("searchable.runs_container").replace("{runtime}", "podman").replace("{image}", image),
                tr("convert.first_run_pulls_large").replace("{image}", image).replace("{size}", "180 MB"),
            );
            for kind in 0..3 {
                for w in [80, 140] {
                    let (title, text) = ("t".to_string(), text.clone());
                    app.dialog = Some(match kind {
                        0 => Dialog::Searchable { title, text, pdfs: vec![] },
                        1 => Dialog::Confirm { title, text, paths: vec![], forever: false },
                        _ => Dialog::Message { title, text },
                    });
                    let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, 30)).unwrap();
                    term.draw(|f| draw(f, &mut app)).unwrap();
                    let buf = term.backend().buffer();
                    let screen: String = (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>() + "\n").collect();
                    assert!(screen.contains("MB)."), "{lang}, dialog {kind} at {w}:\n{screen}");
                }
            }
        }
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn find_makes_lines_only_for_the_rows_in_sight() {
        use coxswain_core::find::{GroupId, Row};
        use coxswain_core::index::Hit;
        let hit = |i: usize| Row::Hit {
            group: GroupId::Names,
            hit: Hit { path: format!("/f{i}").into(), is_dir: false, snippet: i.is_multiple_of(2).then(|| "x".into()), similar: None },
        };
        let rows: Vec<Row> = (0..10_000).map(hit).collect();
        let mut offset = 0;
        // At the top: rows of 2, 1, 2, 1 … lines fill 10 lines with 7 rows (the last one cut).
        assert_eq!(in_sight(&rows, &mut offset, 0, 10), 0..7);
        // The cursor far down: the offset follows it, and only a few rows are made.
        let r = in_sight(&rows, &mut offset, 9_999, 10);
        assert_eq!(offset, 9_993);
        assert_eq!(r, 9_993..10_000);
        // Back up: the offset comes back to the cursor.
        assert_eq!(in_sight(&rows, &mut offset, 5_000, 10), 5_000..5_007);
    }

    #[test]
    fn persian_lam_alef_takes_two_columns() {
        assert_eq!("بالاتر".width(), 6);
        assert_eq!(fit("بالاتر", 8).width(), 8);
        assert_eq!("日本".width(), 4);
    }

    #[test]
    fn keybar_lends_room_to_long_labels() {
        let nums = [1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 2];
        // British English fits: even slots of 7, the last one 10.
        let en = [4, 4, 4, 4, 4, 6, 5, 6, 6, 4, 8];
        assert_eq!(keybar_widths(&nums, &en, 80), [6, 6, 6, 6, 6, 6, 6, 6, 6, 5, 8]);
        // German does not: Hilfe Menü Ansicht Bearbeit. Kopieren RenMov Mkdir Löschen PullDn
        // Beenden Einstellungen. Only the longest are cut, and the bar still fills 80 columns.
        let de = [5, 4, 7, 9, 8, 6, 5, 7, 6, 7, 13];
        let w = keybar_widths(&nums, &de, 80);
        assert_eq!(w.iter().sum::<usize>() + nums.iter().sum::<usize>(), 80);
        let cut = w.iter().zip(&de).filter(|(w, l)| w < l).count();
        assert!(cut <= 3, "{w:?}");
        // Too narrow for anything: no panic, nothing wider than the bar.
        assert!(keybar_widths(&nums, &de, 10).iter().sum::<usize>() <= 10);
    }

    #[test]
    fn features_wrap_to_the_width_and_mark_the_cursor() {
        assert_eq!(wrap("one two three four", 9), ["one two", "three", "four"]);
        assert!(wrap("日本語のテキストはスペースがない", 10).iter().all(|l| l.width() <= 10));
        // A Latin word in Japanese does not leave the line before it short.
        let ja = wrap("はい。F1 を押し、デスクトップアプリでは「ガイドを再表示」を押します", 20);
        assert!(ja[0].width() >= 18 && ja.iter().all(|l| l.width() <= 20), "{ja:?}");
        let list = coxswain_core::features::list(&config::Config::default(), "", false);
        let (lines, at) = feature_lines(&list, 2, true, 60, &config::Theme::nc());
        assert!(lines[at].to_string().contains(&list[2].title), "{}", lines[at]);
        assert!(lines.iter().all(|l| l.width() <= 60));
    }

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

/// Properties of a file or folder, as lines of text: the basics, then ZFS, the package and the
/// flags where the system has them. `snapshots_key` lists the snapshots.
/// `disk_next`: what can be done next with a disk image (`disk_image::next_line`).
pub(crate) fn properties(p: &coxswain_core::fs::Props, snapshots_key: &str, disk_next: &str) -> String {
    let mut v: Vec<(String, String)> = vec![];
    let mut add = |k: &str, val: String| v.push((t!(k), val));
    add("dialogs.location", p.path.parent().map_or(String::new(), |d| d.display().to_string()));
    let kind = t!(&format!("dialogs.kind.{}", p.kind));
    add("dialogs.type", match &p.link_target {
        Some(l) => format!("{kind} -> {}", l.display()),
        None => kind,
    });
    let mut sz = size(p.size);
    if p.kind == "folder" {
        sz = format!("{sz} {}", tn!("dialogs.in_files", p.files));
    }
    add("dialogs.size", sz);
    if let Some(m) = p.modified {
        add("dialogs.modified", date(m));
    }
    if let (Some(mode), Some(uid), Some(gid)) = (p.mode, p.uid, p.gid) {
        let ids = t!("dialogs.owner_ids", "uid" => uid, "gid" => gid);
        add("dialogs.owner", p.owner.as_ref().map_or(ids.clone(), |o| format!("{o} · {ids}")));
        add("dialogs.permissions", format!("{mode:04o}"));
    }
    if let Some(z) = &p.zfs {
        add("props.dataset", z.dataset.clone());
        add("props.mountpoint", z.mountpoint.display().to_string());
        add("props.compression", t!("props.compression_value", "compression" => z.compression, "ratio" => z.compressratio));
        add("props.space", t!("props.space_value", "used" => size(z.used), "free" => size(z.available), "referenced" => size(z.referenced)));
        if z.quota > 0 || z.refquota > 0 {
            let q = |n: u64| if n > 0 { size(n) } else { t!("props.none") };
            add("props.quota", t!("props.quota_value", "quota" => q(z.quota), "refquota" => q(z.refquota)));
        }
        if let Some(n) = p.snapshots {
            add("props.snapshots", t!("props.snapshots_value", "n" => n, "key" => snapshots_key));
        }
    }
    if let Some(pkg) = &p.package {
        add("props.package", t!("props.package_tui", "name" => pkg));
    }
    if let Some(f) = &p.flags {
        add("props.flags", if f.set.is_empty() { t!("props.flags_none") } else { f.set.join(", ") });
    }
    for (k, val) in p.disk_image.iter().chain(&p.facts).flatten() {
        v.push((k.clone(), val.clone()));
    }
    let w = v.iter().map(|(k, _)| k.width()).max().unwrap_or(0);
    let mut out: Vec<String> = v.iter().map(|(k, val)| format!("{k}{} {val}", " ".repeat(w - k.width()))).collect();
    if let Some(f) = &p.flags {
        if f.system {
            out.push(String::new());
            out.push(t!("props.flags_system"));
        }
        if f.can_set && !f.user.is_empty() {
            out.push(t!("props.flags_change_tui"));
        }
    }
    if p.disk_image.is_some() {
        out.push(String::new());
        // Wrapped here: the dialog is as tall as the lines it is given.
        out.extend(wrap(disk_next, 74));
    }
    out.join("\n")
}

#[cfg(test)]
mod sty_tests {
    use super::*;

    /// A theme's `#rrggbb`, read directly, is the colour `Color::from_str` reads.
    #[test]
    fn hex_colours_read_as_ratatui_reads_them() {
        for c in ["#ffffff", "#1A2b3C", "#000000", "red", "", "#12345", "#+12345"] {
            let s = config::Style { fg: c.into(), ..Default::default() };
            assert_eq!(sty(&s).fg, (!c.is_empty()).then(|| Color::from_str(c).ok()).flatten(), "{c}");
        }
    }
}
