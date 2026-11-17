use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct BookMetadata {
    pub author: String,
    pub publisher: String,
    pub date_ym: String,
}

impl BookMetadata {
    /// Load current author, publisher, and date values from `front/cover.tex` and `front/preface.tex`.
    pub fn load_from_project(root: &Path) -> Self {
        let mut meta = Self::default();

        if let Ok(cover) = fs::read_to_string(root.join("front/cover.tex")) {
            if let Some(a) = extract_newcommand_arg(&cover, "\\BookCoverAuthor") {
                meta.author = a;
            }
            if let Some(p) = extract_newcommand_arg(&cover, "\\BookCoverPublisher") {
                meta.publisher = p;
            }
        }
        if meta.author.is_empty() || meta.author.contains("［") {
            if let Ok(tp) = fs::read_to_string(root.join("front/titlepage.tex")) {
                if tp.contains("{\\large 作者\\par}") {
                    if meta.author.is_empty() {
                        meta.author = "［作者署名］".to_string();
                    }
                }
            }
        }
        if let Ok(pref) = fs::read_to_string(root.join("front/preface.tex")) {
            if pref.contains("\\todo{年月}") {
                meta.date_ym = "2026 年 11 月".to_string();
            }
        }
        meta
    }

    /// Apply author, publisher, and year-month back into `front/cover.tex`, `front/titlepage.tex`,
    /// `main.tex` (`pdfauthor`), and `front/preface.tex`.
    pub fn save_to_project(&self, root: &Path) -> anyhow::Result<Vec<String>> {
        let mut updated = Vec::new();
        let author = self.author.trim();
        let publisher = self.publisher.trim();
        let date_ym = self.date_ym.trim();

        let cover_path = root.join("front/cover.tex");
        if cover_path.is_file() {
            let mut content = fs::read_to_string(&cover_path)?;
            if !author.is_empty() {
                content = replace_newcommand_arg(&content, "\\BookCoverAuthor", author);
            }
            if !publisher.is_empty() {
                content = replace_newcommand_arg(&content, "\\BookCoverPublisher", publisher);
            }
            fs::write(&cover_path, content)?;
            updated.push("front/cover.tex".to_string());
        }

        let tp_path = root.join("front/titlepage.tex");
        if tp_path.is_file() && !author.is_empty() && !author.contains('［') {
            let content = fs::read_to_string(&tp_path)?;
            if content.contains("{\\large 作者\\par}") {
                let next = content.replace("{\\large 作者\\par}", &format!("{{\\large {author}\\par}}"));
                fs::write(&tp_path, next)?;
                updated.push("front/titlepage.tex".to_string());
            }
        }

        let main_path = root.join("main.tex");
        if main_path.is_file() && !author.is_empty() && !author.contains('［') {
            let content = fs::read_to_string(&main_path)?;
            let next = content.replace("pdfauthor = {},", &format!("pdfauthor = {{{author}}},"));
            if next != content {
                fs::write(&main_path, next)?;
                updated.push("main.tex".to_string());
            }
        }

        let pref_path = root.join("front/preface.tex");
        if pref_path.is_file() {
            let mut content = fs::read_to_string(&pref_path)?;
            let orig = content.clone();
            if !author.is_empty() && !author.contains('［') {
                content = content.replace("\\todo{作者署名}", author);
            }
            if !date_ym.is_empty() {
                content = content.replace("\\todo{年月}", date_ym);
            }
            if content != orig {
                fs::write(&pref_path, content)?;
                updated.push("front/preface.tex".to_string());
            }
        }

        Ok(updated)
    }
}

fn extract_newcommand_arg(src: &str, cmd: &str) -> Option<String> {
    let needle = format!("\\newcommand{{{cmd}}}{{");
    let pos = src.find(&needle)?;
    let rest = &src[pos + needle.len()..];
    let end = rest.find('}')?;
    Some(rest[..end].to_string())
}

fn replace_newcommand_arg(src: &str, cmd: &str, new_val: &str) -> String {
    let needle = format!("\\newcommand{{{cmd}}}{{");
    if let Some(pos) = src.find(&needle) {
        let start = pos + needle.len();
        if let Some(rel_end) = src[start..].find('}') {
            let mut out = String::with_capacity(src.len());
            out.push_str(&src[..start]);
            out.push_str(new_val);
            out.push_str(&src[start + rel_end..]);
            return out;
        }
    }
    src.to_string()
}
