use crate::{
    ast::{BlockKind, Document, inlines_plain_text},
    parser::parse_latex,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BookError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("CSV error: {0}")]
    Csv(#[from] csv::Error),
    #[error("Chapter not found: {0}")]
    ChapterNotFound(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChapterEntry {
    pub relative_path: String,
    pub full_path: PathBuf,
    pub section_group: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TermEntry {
    pub english: String,
    pub preferred_zh: String,
    pub mainland_zh: String,
    pub avoided_terms: Vec<String>,
    pub symbol: String,
    pub chapter: String,
    pub naer_status: String,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TermIssue {
    pub file: String,
    pub line: usize,
    pub kind: String,
    pub matched: String,
    pub suggestion: String,
    pub context: String,
}

#[derive(Debug, Clone)]
pub struct BookProject {
    pub root_dir: PathBuf,
    pub main_tex: PathBuf,
    pub chapters: Vec<ChapterEntry>,
    pub terms: Vec<TermEntry>,
}

impl BookProject {
    /// Load a book project from a directory containing `main.tex` (or open a single `.tex` file).
    pub fn open(root_or_main: impl AsRef<Path>) -> Result<Self, BookError> {
        let path = root_or_main.as_ref();
        let (root_dir, main_tex) = if path.is_file() {
            (
                path.parent().unwrap_or_else(|| Path::new(".")).to_path_buf(),
                path.to_path_buf(),
            )
        } else {
            (path.to_path_buf(), path.join("main.tex"))
        };

        let mut chapters = Vec::new();
        if main_tex.exists() {
            let main_content = fs::read_to_string(&main_tex)?;
            let mut current_group = "前頁".to_string();
            for line in main_content.lines() {
                let t = line.trim();
                if t.starts_with('%') {
                    continue;
                }
                if t.contains("\\frontmatter") {
                    current_group = "前頁".to_string();
                } else if t.contains("\\mainmatter") {
                    current_group = "正文".to_string();
                } else if t.contains("\\appendix") {
                    current_group = "附錄".to_string();
                } else if t.contains("\\backmatter") {
                    current_group = "後頁".to_string();
                }

                if let Some(rel) = extract_include_target(t) {
                    let rel_tex = if rel.ends_with(".tex") {
                        rel.clone()
                    } else {
                        format!("{}.tex", rel)
                    };
                    let full_path = root_dir.join(&rel_tex);
                    let title = read_chapter_title(&full_path).unwrap_or_else(|| rel_tex.clone());
                    chapters.push(ChapterEntry {
                        relative_path: rel_tex,
                        full_path,
                        section_group: current_group.clone(),
                        title,
                    });
                }
            }
        }

        if chapters.is_empty() {
            if path.is_file() {
                let rel = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("main.tex")
                    .to_string();
                let title = read_chapter_title(path).unwrap_or_else(|| rel.clone());
                chapters.push(ChapterEntry {
                    relative_path: rel,
                    full_path: path.to_path_buf(),
                    section_group: "文件".to_string(),
                    title,
                });
            } else if let Ok(entries) = fs::read_dir(&root_dir) {
                let mut tex_files: Vec<_> = entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "tex"))
                    .collect();
                tex_files.sort();
                for p in tex_files {
                    let rel = p
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("doc.tex")
                        .to_string();
                    let title = read_chapter_title(&p).unwrap_or_else(|| rel.clone());
                    chapters.push(ChapterEntry {
                        relative_path: rel,
                        full_path: p,
                        section_group: "文件".to_string(),
                        title,
                    });
                }
            }
        }

        let terms = load_terms_csv(&root_dir.join("terms").join("terms.csv")).unwrap_or_default();

        Ok(Self {
            root_dir,
            main_tex,
            chapters,
            terms,
        })
    }

    pub fn load_chapter(&self, relative_or_name: &str) -> Result<(ChapterEntry, Document), BookError> {
        let entry = self
            .chapters
            .iter()
            .find(|c| {
                c.relative_path == relative_or_name
                    || c.relative_path.ends_with(relative_or_name)
                    || c.title.contains(relative_or_name)
            })
            .cloned()
            .ok_or_else(|| BookError::ChapterNotFound(relative_or_name.to_string()))?;

        let raw = fs::read_to_string(&entry.full_path)?;
        let mut doc = parse_latex(&raw);
        doc.file_path = Some(entry.relative_path.clone());
        Ok((entry, doc))
    }

    /// Check avoided terms (`避免用詞（檢查用）`), `\todo`, and `\needcite` in raw `.tex` text.
    pub fn check_text_issues(&self, file_label: &str, raw_tex: &str) -> Vec<TermIssue> {
        let mut issues = Vec::new();
        for (idx, line) in raw_tex.lines().enumerate() {
            let line_no = idx + 1;
            let trimmed = line.trim();
            if trimmed.starts_with('%') {
                continue;
            }
            if trimmed.contains("\\todo{") {
                issues.push(TermIssue {
                    file: file_label.to_string(),
                    line: line_no,
                    kind: "待補標記".to_string(),
                    matched: "\\todo".to_string(),
                    suggestion: "請補齊待辦內容".to_string(),
                    context: trimmed.chars().take(80).collect(),
                });
            }
            if trimmed.contains("\\needcite") {
                issues.push(TermIssue {
                    file: file_label.to_string(),
                    line: line_no,
                    kind: "待補引用".to_string(),
                    matched: "\\needcite".to_string(),
                    suggestion: "請補上 \\cite{...}".to_string(),
                    context: trimmed.chars().take(80).collect(),
                });
            }
            for term in &self.terms {
                for bad in &term.avoided_terms {
                    if !bad.is_empty() && line.contains(bad) {
                        issues.push(TermIssue {
                            file: file_label.to_string(),
                            line: line_no,
                            kind: "避免用詞".to_string(),
                            matched: bad.clone(),
                            suggestion: format!("建議改用「{}」（{}）", term.preferred_zh, term.english),
                            context: trimmed.chars().take(80).collect(),
                        });
                    }
                }
            }
        }
        issues
    }

    pub fn search(&self, query: &str, max_hits: usize) -> Vec<(String, usize, String)> {
        let mut hits = Vec::new();
        let q_lower = query.to_lowercase();
        for ch in &self.chapters {
            if let Ok(content) = fs::read_to_string(&ch.full_path) {
                for (i, line) in content.lines().enumerate() {
                    if line.to_lowercase().contains(&q_lower) {
                        hits.push((ch.relative_path.clone(), i + 1, line.trim().to_string()));
                        if hits.len() >= max_hits {
                            return hits;
                        }
                    }
                }
            }
        }
        hits
    }
}

fn extract_include_target(line: &str) -> Option<String> {
    for cmd in ["\\include{", "\\input{"] {
        if let Some(pos) = line.find(cmd) {
            let rest = &line[pos + cmd.len()..];
            if let Some(end) = rest.find('}') {
                return Some(rest[..end].trim().to_string());
            }
        }
    }
    None
}

fn read_chapter_title(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let doc = parse_latex(&text);
    for b in doc.blocks {
        match b.kind {
            BlockKind::Heading { title, .. } => return Some(title),
            BlockKind::Paragraph { inlines } => {
                let p = inlines_plain_text(&inlines);
                if !p.trim().is_empty() {
                    return Some(p.chars().take(32).collect());
                }
            }
            _ => {}
        }
    }
    None
}

pub fn load_terms_csv(path: &Path) -> Result<Vec<TermEntry>, BookError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut rdr = csv::ReaderBuilder::new().has_headers(true).from_path(path)?;
    let mut out = Vec::new();
    for rec in rdr.records() {
        let r = rec?;
        let avoided_raw = r.get(3).unwrap_or("").trim();
        let avoided_terms = avoided_raw
            .split(['、', '／', '/', ',', ';', '；'])
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        out.push(TermEntry {
            english: r.get(0).unwrap_or("").trim().to_string(),
            preferred_zh: r.get(1).unwrap_or("").trim().to_string(),
            mainland_zh: r.get(2).unwrap_or("").trim().to_string(),
            avoided_terms,
            symbol: r.get(4).unwrap_or("").trim().to_string(),
            chapter: r.get(5).unwrap_or("").trim().to_string(),
            naer_status: r.get(6).unwrap_or("").trim().to_string(),
            notes: r.get(7).unwrap_or("").trim().to_string(),
        });
    }
    Ok(out)
}
