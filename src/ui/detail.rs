use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::theme::Theme;
use crate::todo::Task;
use crate::ui::hyperlinks::{LinkTarget, UrlTarget, link_target};
use crate::ui::task_row::{due_label, due_token_style, url_token_style};

pub fn render<'a>(frame: &mut Frame, area: Rect, app: &'a App) -> Vec<UrlTarget<'a>> {
    let theme = app.theme();
    super::fill_bg(frame, area, Style::default().bg(theme.panel));

    let task = app.cur_task();
    // Keep both padding cells inside even very narrow panes.
    let wrap_w = (area.width as usize).saturating_sub(2);
    let (lines, mut targets) = build_lines(theme, task, app.today(), wrap_w);
    targets.extend(super::hyperlinks::line_targets(
        &lines,
        Rect::new(0, 0, area.width, area.height),
        0,
        Style::default().bg(theme.panel).fg(theme.fg),
    ));
    let para = Paragraph::new(lines).style(Style::default().bg(theme.panel).fg(theme.fg));
    frame.render_widget(para, area);
    targets.retain(|target| target.y < area.height);
    for target in &mut targets {
        target.x += area.x;
        target.y += area.y;
    }
    targets
}

fn build_lines<'a>(
    theme: &Theme,
    task: Option<&'a Task>,
    today: &'a str,
    wrap_w: usize,
) -> (Vec<Line<'a>>, Vec<UrlTarget<'a>>) {
    let mut rows: Vec<Line> = Vec::new();
    let mut targets = Vec::new();
    rows.push(line_panel(
        theme,
        vec![Span::styled(
            " DETAIL",
            Style::default().fg(theme.dim).add_modifier(Modifier::BOLD),
        )],
    ));
    rows.push(line_panel(theme, vec![Span::raw(" ")]));
    let Some(t) = task else {
        rows.push(line_panel(
            theme,
            vec![Span::styled(" (no task)", Style::default().fg(theme.dim))],
        ));
        return (rows, targets);
    };

    let priority_value = if let Some(p) = t.priority {
        Span::styled(
            format!("({p})"),
            Style::default()
                .fg(theme.priority_color(p))
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::raw("")
    };
    rows.push(line_panel(
        theme,
        vec![
            Span::styled(" priority  ", Style::default().fg(theme.dim)),
            priority_value,
        ],
    ));
    rows.push(line_panel(
        theme,
        vec![
            Span::styled(" created   ", Style::default().fg(theme.dim)),
            Span::styled(
                t.created_date.as_deref().unwrap_or("—"),
                Style::default().fg(theme.fg),
            ),
        ],
    ));
    if let Some(due) = &t.due {
        rows.push(line_panel(
            theme,
            vec![
                Span::styled(" due       ", Style::default().fg(theme.dim)),
                Span::styled(due.as_str(), Style::default().fg(theme.fg)),
                Span::raw("  "),
                Span::styled(due_label(due, today), Style::default().fg(theme.overdue)),
            ],
        ));
    }
    rows.push(line_panel(
        theme,
        vec![
            Span::styled(" projects  ", Style::default().fg(theme.dim)),
            Span::styled(
                t.projects
                    .iter()
                    .map(|p| format!("+{p}"))
                    .collect::<Vec<_>>()
                    .join(" "),
                Style::default().fg(theme.project),
            ),
        ],
    ));
    rows.push(line_panel(
        theme,
        vec![
            Span::styled(" contexts  ", Style::default().fg(theme.dim)),
            Span::styled(
                t.contexts
                    .iter()
                    .map(|c| format!("@{c}"))
                    .collect::<Vec<_>>()
                    .join(" "),
                Style::default().fg(theme.context),
            ),
        ],
    ));

    for (key, value) in custom_metadata(&t.raw) {
        rows.push(line_panel(
            theme,
            vec![
                Span::raw(" "),
                Span::styled(ellipsize(key, wrap_w), Style::default().fg(theme.dim)),
            ],
        ));
        append_value(&mut rows, &mut targets, value, wrap_w, theme, t.done);
    }

    // Rendering notes line by line
    if !t.notes.is_empty() {
        rows.push(line_panel(
            theme,
            vec![Span::styled(" notes", Style::default().fg(theme.dim))],
        ));
        for note in &t.notes {
            let chunks = wrap_words(note, wrap_w.saturating_sub(4));
            for (i, chunk) in chunks.into_iter().enumerate() {
                let prefix = if i == 0 { "   - " } else { "     " };
                rows.push(line_panel(
                    theme,
                    vec![Span::styled(
                        format!("{prefix}{}", chunk.join(" ")),
                        Style::default().fg(theme.fg),
                    )],
                ))
            }
        }
    }

    if t.done {
        rows.push(line_panel(
            theme,
            vec![
                Span::styled(" done      ", Style::default().fg(theme.dim)),
                Span::styled(
                    t.done_date.as_deref().unwrap_or(""),
                    Style::default().fg(theme.done),
                ),
            ],
        ));
    }
    rows.push(line_panel(theme, vec![Span::raw(" ")]));
    rows.push(line_panel(
        theme,
        vec![Span::styled(
            " RAW",
            Style::default().fg(theme.dim).add_modifier(Modifier::BOLD),
        )],
    ));
    rows.push(line_panel(theme, vec![Span::raw(" ")]));
    let mut state = RawWalk::default();
    for chunk in wrap_words(&t.raw, wrap_w) {
        let mut spans: Vec<Span> = vec![Span::raw(" ")];
        let mut words = chunk.into_iter();
        if let Some(first) = words.next() {
            spans.push(style_raw_token(first, t, today, theme, &mut state));
        }
        for w in words {
            spans.push(Span::raw(" "));
            spans.push(style_raw_token(w, t, today, theme, &mut state));
        }
        rows.push(line_panel(theme, spans));
    }
    (rows, targets)
}

#[derive(Default)]
struct RawWalk {
    done_marker_consumed: bool,
    priority_consumed: bool,
}

fn style_raw_token<'a>(
    token: &'a str,
    task: &Task,
    today: &str,
    theme: &Theme,
    state: &mut RawWalk,
) -> Span<'a> {
    if task.done && !state.done_marker_consumed {
        state.done_marker_consumed = true;
        if token == "x" {
            return Span::styled(token, Style::default().fg(theme.done));
        }
    }
    if !state.priority_consumed
        && let Some(p) = task.priority
        && token.len() == 3
        && token.as_bytes()[0] == b'('
        && token.as_bytes()[1] == p as u8
        && token.as_bytes()[2] == b')'
    {
        state.priority_consumed = true;
        return Span::styled(
            token,
            Style::default()
                .fg(theme.priority_color(p))
                .add_modifier(Modifier::BOLD),
        );
    }
    if let Some(rest) = token.strip_prefix("due:") {
        return Span::styled(token, due_token_style(task.done, rest, today, theme));
    }
    if link_target(token).is_some() {
        return Span::styled(token, url_token_style(task.done, theme));
    }
    if token.len() > 1 && token.starts_with('+') {
        return Span::styled(token, Style::default().fg(theme.project));
    }
    if token.len() > 1 && token.starts_with('@') {
        return Span::styled(token, Style::default().fg(theme.context));
    }
    Span::styled(token, Style::default().fg(theme.fg))
}

fn line_panel<'a>(theme: &Theme, spans: Vec<Span<'a>>) -> Line<'a> {
    Line::from(spans).style(Style::default().bg(theme.panel))
}

/// Wrap `s` to roughly `width` graphemes, returning each output line as a
/// vector of borrowed words. Borrowing avoids the per-frame `String` alloc
/// that the previous `Vec<String>` form forced on every render.
fn wrap_words(s: &str, width: usize) -> Vec<Vec<&str>> {
    let mut out: Vec<Vec<&str>> = Vec::new();
    let mut acc: Vec<&str> = Vec::new();
    let mut acc_len = 0;
    for word in s.split_whitespace() {
        let wlen = word.chars().count();
        let extra = if acc.is_empty() { 0 } else { 1 };
        if acc_len + wlen + extra > width && !acc.is_empty() {
            out.push(std::mem::take(&mut acc));
            acc_len = 0;
        }
        if !acc.is_empty() {
            acc_len += 1;
        }
        acc.push(word);
        acc_len += wlen;
    }
    if !acc.is_empty() {
        out.push(acc);
    }
    out
}

/// Read extension tokens while keeping the existing double-quoted value form
/// together. Reserved fields already have dedicated detail/note handling.
fn custom_metadata(mut raw: &str) -> Vec<(&str, &str)> {
    let mut entries = Vec::new();
    while !raw.is_empty() {
        raw = raw.trim_start();
        let end = raw.find(char::is_whitespace).unwrap_or(raw.len());
        let token = &raw[..end];
        let Some((key, initial)) = token.split_once(':') else {
            raw = &raw[end..];
            continue;
        };
        let (value, consumed) = if let Some(quoted) = raw[key.len() + 1..].strip_prefix('"') {
            if let Some(close) = quoted.find('"') {
                (&quoted[..close], key.len() + 2 + close + 1)
            } else {
                (initial, end)
            }
        } else {
            (initial, end)
        };
        if crate::todo::is_valid_key(key)
            && !value.is_empty()
            && !matches!(link_target(token), Some(LinkTarget::Url(_)))
            && !["due", "rec", "t", "note"]
                .iter()
                .any(|reserved| key.eq_ignore_ascii_case(reserved))
        {
            entries.push((key, value));
        }
        raw = &raw[consumed..];
    }
    entries
}

fn ellipsize(label: &str, width: usize) -> String {
    if label.width() <= width {
        return label.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let mut result = String::new();
    let mut used = 0;
    for grapheme in label.graphemes(true) {
        let next = grapheme.width();
        if used + next > width - 1 {
            break;
        }
        result.push_str(grapheme);
        used += next;
    }
    result.push('…');
    result
}

fn append_value<'a>(
    rows: &mut Vec<Line<'a>>,
    targets: &mut Vec<UrlTarget<'a>>,
    value: &'a str,
    width: usize,
    theme: &Theme,
    done: bool,
) {
    if width == 0 {
        return;
    }
    let mut spans = vec![Span::raw(" ")];
    let mut used = 0;
    for word in value.split_whitespace() {
        if used > 0 {
            if used + 1 + word.width() > width {
                rows.push(line_panel(
                    theme,
                    std::mem::replace(&mut spans, vec![Span::raw(" ")]),
                ));
                used = 0;
            } else {
                spans.push(Span::raw(" "));
                used += 1;
            }
        }
        let target = link_target(word);
        let style = if target.is_some() {
            url_token_style(done, theme)
        } else {
            Style::default().fg(theme.fg)
        }
        .bg(theme.panel);
        let mut start = 0;
        let mut fragment_width = 0;
        for (offset, grapheme) in word.grapheme_indices(true) {
            let cells = grapheme.width();
            if cells > width {
                // A two-cell grapheme cannot fit a one-cell pane.
                if offset > start {
                    push_fragment(
                        &mut spans,
                        targets,
                        &word[start..offset],
                        target,
                        style,
                        used,
                        rows.len(),
                    );
                }
                if used + fragment_width > 0 {
                    rows.push(line_panel(
                        theme,
                        std::mem::replace(&mut spans, vec![Span::raw(" ")]),
                    ));
                }
                push_fragment(&mut spans, targets, "…", target, style, 0, rows.len());
                used = 1;
                start = offset + grapheme.len();
                fragment_width = 0;
                continue;
            }
            if used + fragment_width + cells > width {
                let fragment = &word[start..offset];
                push_fragment(
                    &mut spans,
                    targets,
                    fragment,
                    target,
                    style,
                    used,
                    rows.len(),
                );
                rows.push(line_panel(
                    theme,
                    std::mem::replace(&mut spans, vec![Span::raw(" ")]),
                ));
                used = 0;
                start = offset;
                fragment_width = 0;
            }
            fragment_width += cells;
        }
        let fragment = &word[start..];
        push_fragment(
            &mut spans,
            targets,
            fragment,
            target,
            style,
            used,
            rows.len(),
        );
        used += fragment_width;
    }
    if spans.len() > 1 {
        rows.push(line_panel(theme, spans));
    }
}

#[allow(clippy::too_many_arguments)]
fn push_fragment<'a>(
    spans: &mut Vec<Span<'a>>,
    targets: &mut Vec<UrlTarget<'a>>,
    fragment: &'a str,
    target: Option<LinkTarget<'a>>,
    style: Style,
    used: usize,
    row: usize,
) {
    if fragment.is_empty() {
        return;
    }
    if let Some(target) = target {
        targets.push(UrlTarget {
            x: (used + 1) as u16,
            y: row as u16,
            text: fragment,
            target,
            style,
        });
    }
    spans.push(Span::styled(fragment, style));
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::ui::hyperlinks;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn real_draw_bare_reference_links_list_and_raw_without_changing_text() {
        let reference = "phenome/tuxedo#173";
        let mut app = App::new(
            "/tmp/tuxedo-reference-bare.txt".into(),
            format!("Task {reference}"),
            "2026-10-28".into(),
            Config::default(),
        );
        app.prefs.layout.right = true;
        let mut terminal = Terminal::new(TestBackend::new(110, 40)).unwrap();
        let mut runs = Vec::new();
        terminal
            .draw(|frame| runs = crate::ui::draw(frame, &app))
            .unwrap();
        assert_eq!(runs.len(), 2, "list row and RAW detail must both link");
        assert!(runs.iter().all(|run| run.text == reference));
        let mut bytes = Vec::new();
        hyperlinks::emit_overlay(&mut bytes, &runs).unwrap();
        let output = String::from_utf8(bytes).unwrap();
        assert_eq!(
            output
                .matches("\x1b]8;;https://github.com/phenome/tuxedo/issues/173\x1b\\")
                .count(),
            2
        );
        assert_eq!(
            output.matches("phenome/tuxedo#173\x1b]8;;\x1b\\").count(),
            2
        );
        assert!(
            terminal
                .backend()
                .buffer()
                .content
                .iter()
                .all(|cell| !cell.symbol().contains('\x1b'))
        );
    }

    #[test]
    fn real_draw_rejects_invalid_reference_tokens_on_every_surface() {
        for token in [
            "#173",
            "phenome/tuxedo#0",
            "phenome/tuxedo#000",
            "phenome/tuxedo#١٧٣",
            "phenome/tuxedo#１２３",
            "phenome/tuxedo#-1",
            "phenome/tuxedo#1x",
            "phenome/tuxedo#1#2",
            "a/b/c#173",
            "./phenome/tuxedo#173",
            "/phenome/tuxedo#173",
            "x!phenome/tuxedo#173",
            "(phenome/tuxedo#173)",
            "phenome/tuxedo#173,",
            "phenome//tuxedo#173",
            "-owner/repo#1",
            "owner-/repo#1",
            "own--er/repo#1",
            "owner/..#1",
            "owner/#1",
            "/repo#1",
            "owner/repo#",
            "é/repo#1",
        ] {
            let mut app = App::new(
                "/tmp/tuxedo-reference-invalid.txt".into(),
                format!("Task {token} link:{token}"),
                "2026-10-28".into(),
                Config::default(),
            );
            app.prefs.layout.right = true;
            let mut terminal = Terminal::new(TestBackend::new(160, 60)).unwrap();
            let mut runs = Vec::new();
            terminal
                .draw(|frame| runs = crate::ui::draw(frame, &app))
                .unwrap();
            assert!(runs.is_empty(), "invalid token linked: {token}");
            let mut bytes = Vec::new();
            hyperlinks::emit_overlay(&mut bytes, &runs).unwrap();
            assert!(bytes.is_empty());
        }
    }

    #[test]
    fn wrapped_reference_metadata_keeps_canonical_target_under_help() {
        let reference = "phenome/tuxedo-with-a-long-repository-name#173";
        let canonical = "https://github.com/phenome/tuxedo-with-a-long-repository-name/issues/173";
        let mut app = App::new(
            "/tmp/tuxedo-reference-wrapped.txt".into(),
            format!("Task link:{reference}"),
            "2026-10-28".into(),
            Config::default(),
        );
        app.prefs.layout.right = true;
        for mode in [crate::app::Mode::Normal, crate::app::Mode::Help] {
            app.mode = mode;
            let mut terminal = Terminal::new(TestBackend::new(110, 40)).unwrap();
            let mut runs = Vec::new();
            terminal
                .draw(|frame| runs = crate::ui::draw(frame, &app))
                .unwrap();
            assert!(!runs.is_empty(), "no surviving reference in {mode:?}");
            assert!(runs.iter().all(|run| run.target == link_target(reference)));
            if mode == crate::app::Mode::Normal {
                assert!(runs.len() >= 2);
                assert_eq!(
                    runs.iter().map(|run| run.text.as_str()).collect::<String>(),
                    reference
                );
            } else {
                assert!(runs.iter().all(|run| !run.text.starts_with("phenome/")));
            }
            let mut bytes = Vec::new();
            hyperlinks::emit_overlay(&mut bytes, &runs).unwrap();
            let output = String::from_utf8(bytes).unwrap();
            assert_eq!(
                output
                    .matches(&format!("\x1b]8;;{canonical}\x1b\\"))
                    .count(),
                runs.len()
            );
        }
    }

    #[test]
    fn metadata_preserves_quoted_values_and_excludes_builtins_and_urls() {
        assert_eq!(
            custom_metadata(
                r#"Task due:2026-10-28 rec:1w t:2026-10-01 note:"a note owner:wrong" owner:alice link:https://example.com summary:"many words here" https://standalone.test Note:path empty: bad.key:value"#
            ),
            vec![
                ("owner", "alice"),
                ("link", "https://example.com"),
                ("summary", "many words here")
            ]
        );
    }

    #[test]
    fn labels_fit_cell_width_without_splitting_graphemes() {
        for width in 0..12 {
            let label = ellipsize("e\u{301}界👩‍💻long", width);
            assert!(label.width() <= width);
            if width > 0 && width < "e\u{301}界👩‍💻long".width() {
                assert!(label.ends_with('…'));
            }
        }
        assert_eq!(ellipsize("e\u{301}long", 2), "e\u{301}…");
        assert_eq!(ellipsize("short", 5), "short");
    }

    #[test]
    fn values_wrap_without_losing_text_even_in_narrow_panes() {
        let app = App::new(
            "/tmp/tuxedo-detail-unit.txt".into(),
            "Task".into(),
            "2026-10-28".into(),
            Config::default(),
        );
        for width in 1..12 {
            let mut rows = Vec::new();
            let mut targets = Vec::new();
            append_value(
                &mut rows,
                &mut targets,
                "alpha beta verylongword",
                width,
                app.theme(),
                false,
            );
            let text: String = rows
                .iter()
                .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
                .collect();
            assert_eq!(
                text.split_whitespace().collect::<String>(),
                "alphabetaverylongword"
            );
            assert!(rows.iter().all(|line| line.width() <= width + 1));
        }
        let mut rows = Vec::new();
        append_value(&mut rows, &mut Vec::new(), "界a", 1, app.theme(), false);
        assert!(rows.iter().all(|line| line.width() <= 2));
    }

    #[test]
    fn real_draw_links_every_wrapped_fragment_to_the_full_url() {
        let url = "https://example.com/a/long/path/that/must/wrap";
        let mut app = App::new(
            "/tmp/tuxedo-detail-links.txt".into(),
            format!("Task owner:alice link:{url}"),
            "2026-10-28".into(),
            Config::default(),
        );
        app.prefs.layout.right = true;
        let mut terminal = Terminal::new(TestBackend::new(110, 40)).unwrap();
        let mut runs = Vec::new();
        terminal
            .draw(|frame| runs = crate::ui::draw(frame, &app))
            .unwrap();
        let fragments: Vec<_> = runs
            .iter()
            .filter(|run| run.target == Some(LinkTarget::Url(url)))
            .collect();
        assert!(fragments.len() >= 2);
        assert_eq!(
            fragments
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>(),
            url
        );
        let mut bytes = Vec::new();
        hyperlinks::emit_overlay(&mut bytes, &runs).unwrap();
        let output = String::from_utf8(bytes).unwrap();
        assert_eq!(
            output.matches(&format!("\x1b]8;;{url}\x1b\\")).count(),
            fragments.len()
        );
        let buffer = terminal.backend().buffer();
        let visible: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
        assert!(visible.contains("owner"));
        assert!(visible.contains("alice"));
        assert!(visible.contains("link"));
    }

    #[test]
    fn unicode_links_keep_full_targets_when_help_obscures_their_prefixes() {
        let url = "https://example.com/界/long/path/that/must/wrap";
        let mut app = App::new(
            "/tmp/tuxedo-detail-unicode-links.txt".into(),
            format!("Task link:{url}"),
            "2026-10-28".into(),
            Config::default(),
        );
        app.prefs.layout.right = true;
        for mode in [crate::app::Mode::Normal, crate::app::Mode::Help] {
            app.mode = mode;
            let mut terminal = Terminal::new(TestBackend::new(110, 40)).unwrap();
            let mut runs = Vec::new();
            terminal
                .draw(|frame| runs = crate::ui::draw(frame, &app))
                .unwrap();
            assert!(!runs.is_empty(), "expected surviving URL cells in {mode:?}");
            assert!(
                runs.iter()
                    .all(|run| run.target == Some(LinkTarget::Url(url)))
            );
            if mode == crate::app::Mode::Normal {
                assert_eq!(
                    runs.iter().map(|run| run.text.as_str()).collect::<String>(),
                    url
                );
                assert!(runs.iter().any(|run| run.text.contains('界')));
            } else {
                assert!(runs.iter().all(|run| !run.text.starts_with("https://")));
            }
            let mut bytes = Vec::new();
            hyperlinks::emit_overlay(&mut bytes, &runs).unwrap();
            let output = String::from_utf8(bytes).unwrap();
            assert_eq!(
                output.matches(&format!("\x1b]8;;{url}\x1b\\")).count(),
                runs.len()
            );
        }
    }
}
