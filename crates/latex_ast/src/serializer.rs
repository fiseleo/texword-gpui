use crate::ast::*;

/// Serialize a `Document` back to clean XeLaTeX source code.
pub fn serialize_latex(doc: &Document) -> String {
    let mut parts = Vec::with_capacity(doc.blocks.len());
    for block in &doc.blocks {
        parts.push(serialize_block(block));
    }
    let mut out = parts.join("\n\n");
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

pub fn serialize_block(block: &Block) -> String {
    serialize_block_kind(&block.kind)
}

pub fn serialize_block_kind(kind: &BlockKind) -> String {
    match kind {
        BlockKind::Heading {
            level,
            starred,
            title,
            label,
        } => {
            let cmd = match level {
                HeadingLevel::Chapter => "\\chapter",
                HeadingLevel::Section => "\\section",
                HeadingLevel::Subsection => "\\subsection",
                HeadingLevel::Subsubsection => "\\subsubsection",
            };
            let star = if *starred { "*" } else { "" };
            match label {
                Some(l) if !l.is_empty() => format!("{cmd}{star}{{{title}}}\\label{{{l}}}"),
                _ => format!("{cmd}{star}{{{title}}}"),
            }
        }
        BlockKind::Paragraph { inlines } => serialize_inlines(inlines),
        BlockKind::MathBlock { env, tex, label } => {
            if let Some(env_name) = env.env_name() {
                let mut lines = vec![format!("\\begin{{{}}}", env_name)];
                for l in tex.lines() {
                    lines.push(format!("  {}", l));
                }
                if let Some(lbl) = label
                    && !lbl.is_empty()
                {
                    lines.push(format!("  \\label{{{}}}", lbl));
                }
                lines.push(format!("\\end{{{}}}", env_name));
                lines.join("\n")
            } else {
                format!("\\[\n  {}\n\\]", tex)
            }
        }
        BlockKind::Figure {
            path,
            caption,
            source,
            label,
            has_tikzpicture: _,
            raw,
        } => {
            if !raw.trim().is_empty() {
                raw.clone()
            } else {
                let mut lines = vec!["\\begin{figure}[tbp]".to_string(), "  \\centering".to_string()];
                if let Some(p) = path {
                    lines.push(format!("  \\includegraphics{{{}}}", p));
                }
                if let Some(cap) = caption {
                    if let Some(src) = source {
                        lines.push(format!("  \\caption{{{}\\source{{{}}}}}", cap, src));
                    } else {
                        lines.push(format!("  \\caption{{{}}}", cap));
                    }
                }
                if let Some(lbl) = label {
                    lines.push(format!("  \\label{{{}}}", lbl));
                }
                lines.push("\\end{figure}".to_string());
                lines.join("\n")
            }
        }
        BlockKind::Callout {
            kind,
            title,
            blocks,
        } => {
            let env = kind.env_name();
            let header = match (kind, title) {
                (CalloutKind::Casestudy, Some(t)) if !t.is_empty() => {
                    format!("\\begin{{{}}}{{{}}}", env, t)
                }
                (_, Some(t)) if !t.is_empty() => format!("\\begin{{{}}}[{}]", env, t),
                _ => format!("\\begin{{{}}}", env),
            };
            let mut inner = Vec::new();
            for b in blocks {
                inner.push(serialize_block(b));
            }
            if inner.is_empty() {
                format!("{}\n\\end{{{}}}", header, env)
            } else {
                format!("{}\n{}\n\\end{{{}}}", header, inner.join("\n\n"), env)
            }
        }
        BlockKind::List { ordered, items } => {
            let env = if *ordered { "enumerate" } else { "itemize" };
            let mut lines = vec![format!("\\begin{{{}}}", env)];
            for item in items {
                lines.push(format!("  \\item {}", serialize_inlines(item)));
            }
            lines.push(format!("\\end{{{}}}", env));
            lines.join("\n")
        }
        BlockKind::Comment { lines } => lines.join("\n"),
        BlockKind::RawLatex { raw, .. } => raw.clone(),
    }
}

pub fn serialize_inlines(inlines: &[Inline]) -> String {
    let mut out = String::new();
    for item in inlines {
        match item {
            Inline::Text { text } => out.push_str(text),
            Inline::Math { tex } => {
                out.push('$');
                out.push_str(tex);
                out.push('$');
            }
            Inline::Term { zh, en } => out.push_str(&format!("\\term{{{}}}{{{}}}", zh, en)),
            Inline::SiUnit { value, unit } => {
                out.push_str(&format!("\\SI{{{}}}{{{}}}", value, unit))
            }
            Inline::Chem { formula } => out.push_str(&format!("\\ce{{{}}}", formula)),
            Inline::Bold { children } => {
                out.push_str(&format!("\\textbf{{{}}}", serialize_inlines(children)))
            }
            Inline::Italic { children } => {
                out.push_str(&format!("\\emph{{{}}}", serialize_inlines(children)))
            }
            Inline::Ref { ref_cmd, target } => {
                out.push_str(&format!("\\{}{{{}}}", ref_cmd, target))
            }
            Inline::Cite { keys } => out.push_str(&format!("\\cite{{{}}}", keys)),
            Inline::Todo { text } => out.push_str(&format!("\\todo{{{}}}", text)),
            Inline::NeedCite => out.push_str("\\needcite"),
            Inline::Russian { text } => out.push_str(&format!("\\ru{{{}}}", text)),
            Inline::Macro { raw } => out.push_str(raw),
        }
    }
    out
}
