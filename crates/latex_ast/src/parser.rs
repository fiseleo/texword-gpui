use crate::ast::*;

/// Parse a LaTeX `.tex` source file into a structured `Document`.
/// Unknown environments (e.g., `table`, `tikzpicture`, custom preamble) are preserved as `RawLatex`.
pub fn parse_latex(input: &str) -> Document {
    let normalized = input.replace("\r\n", "\n");
    let lines: Vec<&str> = normalized.split('\n').collect();
    let blocks = parse_blocks(&lines);
    Document::new(blocks)
}

fn parse_blocks(lines: &[&str]) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        // Skip empty separator lines
        if trimmed.is_empty() {
            i += 1;
            continue;
        }

        // Consecutive pure-comment lines -> Comment block
        if trimmed.starts_with('%') {
            let mut comment_lines = Vec::new();
            while i < lines.len() && lines[i].trim().starts_with('%') {
                comment_lines.push(lines[i].to_string());
                i += 1;
            }
            blocks.push(Block::new(BlockKind::Comment {
                lines: comment_lines,
            }));
            continue;
        }

        // Heading line: \chapter, \section, \subsection, \subsubsection
        if let Some(heading) = try_parse_heading(trimmed) {
            blocks.push(Block::new(heading));
            i += 1;
            continue;
        }

        // Display math \[ ... \]
        if trimmed.starts_with("\\[") {
            let mut collected = Vec::new();
            let mut found_end = false;
            if trimmed.ends_with("\\]") && trimmed.len() >= 4 {
                let inner = trimmed[2..trimmed.len() - 2].trim().to_string();
                blocks.push(Block::new(BlockKind::MathBlock {
                    env: MathEnv::DisplayBracket,
                    tex: inner,
                    label: None,
                }));
                i += 1;
                continue;
            }
            collected.push(trimmed.trim_start_matches("\\[").to_string());
            i += 1;
            while i < lines.len() {
                let l = lines[i];
                if let Some(pos) = l.find("\\]") {
                    collected.push(l[..pos].to_string());
                    found_end = true;
                    i += 1;
                    break;
                }
                collected.push(l.to_string());
                i += 1;
            }
            if found_end {
                blocks.push(Block::new(BlockKind::MathBlock {
                    env: MathEnv::DisplayBracket,
                    tex: collected.join("\n").trim().to_string(),
                    label: None,
                }));
            } else {
                blocks.push(Block::new(BlockKind::RawLatex {
                    env_name: None,
                    raw: collected.join("\n"),
                }));
            }
            continue;
        }

        // Environment \begin{env}
        if let Some(env_name) = extract_begin_env(trimmed) {
            let end_marker = format!("\\end{{{}}}", env_name);
            let start_idx = i;
            let mut depth = 0usize;
            let mut end_idx = None;

            let begin_marker = format!("\\begin{{{}}}", env_name);
            let mut j = i;
            while j < lines.len() {
                let l = lines[j];
                if l.contains(&begin_marker) {
                    depth += l.matches(&begin_marker).count();
                }
                if l.contains(&end_marker) {
                    let ends = l.matches(&end_marker).count();
                    depth = depth.saturating_sub(ends);
                    if depth == 0 {
                        end_idx = Some(j);
                        break;
                    }
                }
                j += 1;
            }

            if let Some(end_line) = end_idx {
                let raw = lines[start_idx..=end_line].join("\n");
                let body_lines = if end_line > start_idx + 1 {
                    &lines[start_idx + 1..end_line]
                } else {
                    &[]
                };

                let block_kind = parse_environment(&env_name, lines[start_idx], body_lines, raw);
                blocks.push(Block::new(block_kind));
                i = end_line + 1;
                continue;
            }
        }

        // Otherwise, accumulate paragraph lines until blank line, comment, heading, or \begin{
        let mut para_lines = Vec::new();
        while i < lines.len() {
            let t = lines[i].trim();
            if t.is_empty()
                || t.starts_with('%')
                || try_parse_heading(t).is_some()
                || extract_begin_env(t).is_some()
                || t.starts_with("\\[")
            {
                break;
            }
            para_lines.push(lines[i]);
            i += 1;
        }
        let joined = para_lines.join("\n");
        let inlines = parse_inlines(&joined);
        blocks.push(Block::new(BlockKind::Paragraph { inlines }));
    }

    blocks
}

fn try_parse_heading(line: &str) -> Option<BlockKind> {
    let cmds = [
        ("\\chapter", HeadingLevel::Chapter),
        ("\\section", HeadingLevel::Section),
        ("\\subsection", HeadingLevel::Subsection),
        ("\\subsubsection", HeadingLevel::Subsubsection),
    ];

    for (cmd, level) in cmds {
        if let Some(after) = line.strip_prefix(cmd) {
            let (starred, rest) = if let Some(r) = after.strip_prefix('*') {
                (true, r.trim_start())
            } else {
                (false, after.trim_start())
            };
            if let Some((title, tail)) = read_braced_arg(rest) {
                let tail = tail.trim();
                let label = if let Some(lbl_rest) = tail.strip_prefix("\\label") {
                    read_braced_arg(lbl_rest.trim_start()).map(|(l, _)| l)
                } else {
                    None
                };
                return Some(BlockKind::Heading {
                    level,
                    starred,
                    title,
                    label,
                });
            }
        }
    }
    None
}

fn extract_begin_env(line: &str) -> Option<String> {
    let rest = line.strip_prefix("\\begin{")?;
    let close = rest.find('}')?;
    Some(rest[..close].to_string())
}

fn parse_environment(env: &str, header_line: &str, body_lines: &[&str], raw: String) -> BlockKind {
    match env {
        "equation" | "equation*" | "align" | "align*" => {
            let math_env = match env {
                "equation" => MathEnv::Equation,
                "equation*" => MathEnv::EquationStar,
                "align" => MathEnv::Align,
                _ => MathEnv::AlignStar,
            };
            let mut label = None;
            let mut cleaned = Vec::new();
            for line in body_lines {
                let t = line.trim();
                if let Some(after_lbl) = t.strip_prefix("\\label{")
                    && let Some(end) = after_lbl.find('}')
                {
                    label = Some(after_lbl[..end].to_string());
                    continue;
                }
                cleaned.push(*line);
            }
            BlockKind::MathBlock {
                env: math_env,
                tex: cleaned.join("\n").trim().to_string(),
                label,
            }
        }
        "figure" => parse_figure_env(body_lines, raw),
        "itemize" | "enumerate" => {
            let ordered = env == "enumerate";
            let body = body_lines.join("\n");
            let items = parse_list_items(&body);
            BlockKind::List { ordered, items }
        }
        _ => {
            if let Some(kind) = CalloutKind::from_env(env) {
                let title = extract_callout_title(header_line, env);
                let inner_blocks = parse_blocks(body_lines);
                BlockKind::Callout {
                    kind,
                    title,
                    blocks: inner_blocks,
                }
            } else {
                BlockKind::RawLatex {
                    env_name: Some(env.to_string()),
                    raw,
                }
            }
        }
    }
}

fn extract_callout_title(header_line: &str, env: &str) -> Option<String> {
    let prefix = format!("\\begin{{{}}}", env);
    let after = header_line.trim().strip_prefix(&prefix)?.trim();
    if let Some(rest) = after.strip_prefix('[') {
        let close = rest.find(']')?;
        return Some(rest[..close].to_string());
    }
    if let Some((arg, _)) = read_braced_arg(after) {
        return Some(arg);
    }
    None
}

fn parse_figure_env(body_lines: &[&str], raw: String) -> BlockKind {
    let body = body_lines.join("\n");
    let has_tikzpicture = body.contains("\\begin{tikzpicture}");
    let path = extract_macro_first_arg(&body, "\\includegraphics");
    let label = extract_macro_first_arg(&body, "\\label");
    let caption_full = extract_macro_first_arg(&body, "\\caption");
    let (caption, source) = if let Some(cap) = caption_full {
        if let Some(idx) = cap.find("\\source{") {
            let main_cap = cap[..idx].trim().to_string();
            let src = read_braced_arg(&cap[idx + "\\source".len()..]).map(|(s, _)| s);
            (Some(main_cap), src)
        } else {
            (Some(cap), None)
        }
    } else {
        (None, None)
    };

    BlockKind::Figure {
        path,
        caption,
        source,
        label,
        has_tikzpicture,
        raw,
    }
}

fn extract_macro_first_arg(haystack: &str, cmd: &str) -> Option<String> {
    let pos = haystack.find(cmd)?;
    let mut rest = haystack[pos + cmd.len()..].trim_start();
    // Skip optional [...] argument if present
    if rest.starts_with('[') {
        if let Some(end_bracket) = rest.find(']') {
            rest = rest[end_bracket + 1..].trim_start();
        }
    }
    read_braced_arg(rest).map(|(arg, _)| arg)
}

fn parse_list_items(body: &str) -> Vec<Vec<Inline>> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut started = false;

    for line in body.lines() {
        let t = line.trim();
        if let Some(after) = t.strip_prefix("\\item") {
            if started {
                items.push(parse_inlines(current.trim()));
                current.clear();
            }
            started = true;
            current.push_str(after.trim_start());
        } else if started {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(t);
        }
    }
    if started && !current.trim().is_empty() {
        items.push(parse_inlines(current.trim()));
    }
    items
}

/// Parse inline LaTeX content inside paragraphs and list items.
pub fn parse_inlines(input: &str) -> Vec<Inline> {
    let mut out = Vec::new();
    let mut text_buf = String::new();
    let bytes = input.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        let rest = &input[i..];

        // Escaped dollar \$
        if rest.starts_with("\\$") {
            text_buf.push_str("\\$");
            i += 2;
            continue;
        }

        // Inline math $...$
        if rest.starts_with('$') && !rest.starts_with("$$") {
            if let Some(end_rel) = rest[1..].find('$') {
                flush_text(&mut text_buf, &mut out);
                let math = &rest[1..1 + end_rel];
                out.push(Inline::Math {
                    tex: math.to_string(),
                });
                i += 1 + end_rel + 1;
                continue;
            }
        }

        // Macros starting with '\'
        if rest.starts_with('\\') {
            if let Some((inline, consumed)) = try_parse_inline_macro(rest) {
                flush_text(&mut text_buf, &mut out);
                out.push(inline);
                i += consumed;
                continue;
            }
        }

        // Regular UTF-8 char
        let ch = rest.chars().next().unwrap();
        text_buf.push(ch);
        i += ch.len_utf8();
    }

    flush_text(&mut text_buf, &mut out);
    out
}

fn flush_text(buf: &mut String, out: &mut Vec<Inline>) {
    if !buf.is_empty() {
        out.push(Inline::Text {
            text: std::mem::take(buf),
        });
    }
}

fn try_parse_inline_macro(rest: &str) -> Option<(Inline, usize)> {
    if rest.starts_with("\\needcite") && !is_ident_char_after(rest, "\\needcite".len()) {
        return Some((Inline::NeedCite, "\\needcite".len()));
    }

    // Two-arg macros: \term{zh}{en}, \SI{val}{unit}
    if let Some(after) = strip_macro_name(rest, "\\term")
        && let Some((zh, r1)) = read_braced_arg(after)
        && let Some((en, r2)) = read_braced_arg(r1)
    {
        let consumed = rest.len() - r2.len();
        return Some((Inline::Term { zh, en }, consumed));
    }

    if let Some(after) = strip_macro_name(rest, "\\SI")
        && let Some((value, r1)) = read_braced_arg(after)
        && let Some((unit, r2)) = read_braced_arg(r1)
    {
        let consumed = rest.len() - r2.len();
        return Some((Inline::SiUnit { value, unit }, consumed));
    }

    // One-arg macros
    for (cmd, builder) in [
        ("\\ce", "chem"),
        ("\\textbf", "bold"),
        ("\\emph", "italic"),
        ("\\textit", "italic"),
        ("\\cite", "cite"),
        ("\\todo", "todo"),
        ("\\ru", "ru"),
        ("\\figref", "ref"),
        ("\\tabref", "ref"),
        ("\\eqnref", "ref"),
        ("\\chapref", "ref"),
        ("\\secref", "ref"),
        ("\\ref", "ref"),
    ] {
        if let Some(after) = strip_macro_name(rest, cmd)
            && let Some((arg, r1)) = read_braced_arg(after)
        {
            let consumed = rest.len() - r1.len();
            let inline = match builder {
                "chem" => Inline::Chem { formula: arg },
                "bold" => Inline::Bold {
                    children: parse_inlines(&arg),
                },
                "italic" => Inline::Italic {
                    children: parse_inlines(&arg),
                },
                "cite" => Inline::Cite { keys: arg },
                "todo" => Inline::Todo { text: arg },
                "ru" => Inline::Russian { text: arg },
                "ref" => Inline::Ref {
                    ref_cmd: cmd.trim_start_matches('\\').to_string(),
                    target: arg,
                },
                _ => unreachable!(),
            };
            return Some((inline, consumed));
        }
    }

    None
}

fn strip_macro_name<'a>(input: &'a str, cmd: &str) -> Option<&'a str> {
    let after = input.strip_prefix(cmd)?;
    if is_ident_char_after(input, cmd.len()) {
        return None;
    }
    Some(after.trim_start())
}

fn is_ident_char_after(s: &str, offset: usize) -> bool {
    s.get(offset..)
        .and_then(|r| r.chars().next())
        .is_some_and(|c| c.is_ascii_alphabetic())
}

/// Read a balanced `{...}` group from the start of `input`, returning `(inner, remaining)`.
pub fn read_braced_arg(input: &str) -> Option<(String, &str)> {
    let input = input.trim_start();
    let mut chars = input.char_indices();
    let (_, first) = chars.next()?;
    if first != '{' {
        return None;
    }
    let mut depth = 1usize;
    let mut escaped = false;
    for (idx, ch) in chars {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((input[1..idx].to_string(), &input[idx + 1..]));
                }
            }
            _ => {}
        }
    }
    None
}
