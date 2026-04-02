use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

/// Parse markdown text into styled ratatui Lines.
pub fn render_markdown(raw: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut in_code_block = false;

    for line in raw.lines() {
        if line.starts_with("```") {
            in_code_block = !in_code_block;
            if in_code_block {
                lines.push(Line::from(Span::styled(
                    "─".repeat(40),
                    Style::default().fg(Color::DarkGray),
                )));
            } else {
                lines.push(Line::from(Span::styled(
                    "─".repeat(40),
                    Style::default().fg(Color::DarkGray),
                )));
            }
            continue;
        }

        if in_code_block {
            lines.push(Line::from(Span::styled(
                format!("  {}", line),
                Style::default().fg(Color::Green),
            )));
            continue;
        }

        // Headings
        if line.starts_with("### ") {
            lines.push(Line::from(Span::styled(
                line[4..].to_string(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if line.starts_with("## ") {
            lines.push(Line::from(Span::styled(
                line[3..].to_string(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if line.starts_with("# ") {
            lines.push(Line::from(Span::styled(
                line[2..].to_string(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            )));
            continue;
        }

        // Horizontal rule
        if line.trim() == "---" || line.trim() == "***" || line.trim() == "___" {
            lines.push(Line::from(Span::styled(
                "─".repeat(40),
                Style::default().fg(Color::DarkGray),
            )));
            continue;
        }

        // Checkbox list items
        if line.starts_with("- [x] ") || line.starts_with("- [X] ") {
            lines.push(Line::from(vec![
                Span::styled(
                    "  [x] ",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    line[6..].to_string(),
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::CROSSED_OUT),
                ),
            ]));
            continue;
        }
        if line.starts_with("- [ ] ") {
            lines.push(Line::from(vec![
                Span::styled("  [ ] ", Style::default().fg(Color::White)),
                Span::raw(line[6..].to_string()),
            ]));
            continue;
        }

        // Unordered list
        if line.starts_with("- ") || line.starts_with("* ") {
            lines.push(Line::from(vec![
                Span::styled("  • ", Style::default().fg(Color::Cyan)),
                Span::raw(parse_inline_spans(&line[2..])),
            ]));
            continue;
        }

        // Blockquote
        if line.starts_with("> ") {
            lines.push(Line::from(Span::styled(
                format!("  │ {}", &line[2..]),
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            )));
            continue;
        }

        // Empty line
        if line.trim().is_empty() {
            lines.push(Line::from(""));
            continue;
        }

        // Regular text with inline formatting
        lines.push(render_inline(line));
    }

    lines
}

/// Render a line with inline **bold**, *italic*, and `code` spans.
fn render_inline(line: &str) -> Line<'static> {
    let mut spans = Vec::new();
    let mut chars = line.chars().peekable();
    let mut buf = String::new();

    while let Some(c) = chars.next() {
        match c {
            '`' => {
                if !buf.is_empty() {
                    spans.push(Span::raw(buf.clone()));
                    buf.clear();
                }
                let mut code = String::new();
                for ch in chars.by_ref() {
                    if ch == '`' {
                        break;
                    }
                    code.push(ch);
                }
                spans.push(Span::styled(
                    code,
                    Style::default().fg(Color::Green),
                ));
            }
            '*' if chars.peek() == Some(&'*') => {
                chars.next(); // consume second *
                if !buf.is_empty() {
                    spans.push(Span::raw(buf.clone()));
                    buf.clear();
                }
                let mut bold = String::new();
                loop {
                    match chars.next() {
                        Some('*') if chars.peek() == Some(&'*') => {
                            chars.next();
                            break;
                        }
                        Some(ch) => bold.push(ch),
                        None => break,
                    }
                }
                spans.push(Span::styled(
                    bold,
                    Style::default().add_modifier(Modifier::BOLD),
                ));
            }
            '*' => {
                if !buf.is_empty() {
                    spans.push(Span::raw(buf.clone()));
                    buf.clear();
                }
                let mut italic = String::new();
                for ch in chars.by_ref() {
                    if ch == '*' {
                        break;
                    }
                    italic.push(ch);
                }
                spans.push(Span::styled(
                    italic,
                    Style::default().add_modifier(Modifier::ITALIC),
                ));
            }
            _ => buf.push(c),
        }
    }

    if !buf.is_empty() {
        spans.push(Span::raw(buf));
    }

    Line::from(spans)
}

/// Simple inline parse that returns a plain string (for list items etc.)
fn parse_inline_spans(s: &str) -> String {
    // For now just strip markers and return plain text
    s.replace("**", "").replace('*', "").replace('`', "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_headings() {
        let lines = render_markdown("# Title\n## Subtitle\n### Small");
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn test_code_block() {
        let lines = render_markdown("```\nfn main() {}\n```");
        assert_eq!(lines.len(), 3); // open bar, code line, close bar
    }

    #[test]
    fn test_list_items() {
        let lines = render_markdown("- item one\n- item two");
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn test_checkboxes() {
        let lines = render_markdown("- [x] done\n- [ ] todo");
        assert_eq!(lines.len(), 2);
    }
}
