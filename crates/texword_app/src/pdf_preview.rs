use std::path::{Path, PathBuf};
use std::process::Command;

/// Locate directory containing Poppler `pdftoppm` and `pdfinfo` (PATH or MiKTeX bin dir).
pub fn find_poppler_bin_dir() -> Option<PathBuf> {
    let exe_name = if cfg!(target_os = "windows") {
        "pdftoppm.exe"
    } else {
        "pdftoppm"
    };

    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            if dir.join(exe_name).is_file() {
                return Some(dir);
            }
        }
    }

    if cfg!(target_os = "windows") {
        let mut candidates = Vec::new();
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            let p = PathBuf::from(&local);
            candidates.push(p.join(r"Programs\MiKTeX\miktex\bin\x64"));
            candidates.push(p.join(r"Programs\MiKTeX 2.9\miktex\bin\x64"));
        }
        if let Ok(pf) = std::env::var("ProgramFiles") {
            let p = PathBuf::from(&pf);
            candidates.push(p.join(r"MiKTeX\miktex\bin\x64"));
            candidates.push(p.join(r"MiKTeX 2.9\miktex\bin\x64"));
        }
        for dir in candidates {
            if dir.join(exe_name).is_file() {
                return Some(dir);
            }
        }
    }

    None
}

pub fn get_pdf_page_count(pdf_path: &Path) -> Result<usize, String> {
    if !pdf_path.exists() {
        return Err(format!("找不到 PDF 檔案：{}", pdf_path.display()));
    }
    let bin_dir = find_poppler_bin_dir()
        .ok_or_else(|| "找不到 pdfinfo / pdftoppm（請先安裝 MiKTeX 或 Poppler）".to_string())?;
    let exe = bin_dir.join(if cfg!(target_os = "windows") {
        "pdfinfo.exe"
    } else {
        "pdfinfo"
    });
    let out = Command::new(exe)
        .arg(pdf_path)
        .output()
        .map_err(|e| format!("無法執行 pdfinfo：{e}"))?;

    if !out.status.success() {
        return Err("讀取 PDF 頁數失敗".to_string());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Pages:") {
            if let Ok(n) = rest.trim().parse::<usize>() {
                return Ok(n.max(1));
            }
        }
    }
    Ok(1)
}

pub fn render_pdf_page_png(pdf_path: &Path, page: usize, dpi: u32) -> Result<Vec<u8>, String> {
    if !pdf_path.exists() {
        return Err(format!("找不到 PDF 檔案：{}", pdf_path.display()));
    }
    let bin_dir = find_poppler_bin_dir()
        .ok_or_else(|| "找不到 pdftoppm（請先安裝 MiKTeX 或 Poppler）".to_string())?;
    let exe = bin_dir.join(if cfg!(target_os = "windows") {
        "pdftoppm.exe"
    } else {
        "pdftoppm"
    });

    let unique = format!(
        "texword_preview_{}_{}_{}",
        std::process::id(),
        page,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );
    let prefix = std::env::temp_dir().join(&unique);
    let png_path = std::env::temp_dir().join(format!("{unique}.png"));

    let out = Command::new(exe)
        .args([
            "-png",
            "-singlefile",
            "-r",
            &dpi.to_string(),
            "-f",
            &page.to_string(),
            "-l",
            &page.to_string(),
        ])
        .arg(pdf_path)
        .arg(&prefix)
        .output()
        .map_err(|e| format!("無法執行 pdftoppm：{e}"))?;

    if !out.status.success() || !png_path.exists() {
        let _ = std::fs::remove_file(&png_path);
        return Err(format!(
            "PDF 頁面算圖失敗（page={page}）：{}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }

    let bytes = std::fs::read(&png_path).map_err(|e| format!("讀取算圖結果失敗：{e}"))?;
    let _ = std::fs::remove_file(&png_path);
    Ok(bytes)
}

/// Read chapter entries from `main.toc` (`title -> physical page estimate`).
pub fn estimate_chapter_pdf_pages(root_dir: &Path) -> Vec<(String, usize)> {
    let toc_path = root_dir.join("main.toc");
    let Ok(raw) = std::fs::read_to_string(&toc_path) else {
        return Vec::new();
    };

    // Rough frontmatter offset: count roman-numeral chapters + TOC pages (~10 pages default in rocketbook)
    let mut frontmatter_offset = 10usize;
    let mut result = Vec::new();
    for line in raw.lines() {
        if !line.starts_with("\\contentsline {chapter}") {
            continue;
        }
        // Format: \contentsline {chapter}{\numberline {0}安全、法規與團隊}{1}{chapter.0}%
        let parts: Vec<&str> = line.rsplitn(4, '{').collect();
        // parts[0] = "chapter.0}%", parts[1] = "1}"
        if parts.len() >= 2 {
            let page_str = parts[1].trim_end_matches('}').trim();
            if let Ok(logical_page) = page_str.parse::<usize>() {
                // Extract title segment
                let title_clean = line
                    .replace("\\numberline ", "")
                    .chars()
                    .filter(|c| !matches!(c, '{' | '}'))
                    .collect::<String>();
                if logical_page == 1 && frontmatter_offset == 10 {
                    // Keep reasonable default frontmatter offset
                    frontmatter_offset = 10;
                }
                result.push((title_clean, logical_page + frontmatter_offset));
            }
        }
    }
    result
}
