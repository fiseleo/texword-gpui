use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct CompileOutcome {
    pub success: bool,
    pub summary: String,
    pub details: String,
}

/// Decode Windows console output (UTF-8 first, falling back to CP950/Big5 on Traditional Chinese Windows).
pub fn decode_console_bytes(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    if let Ok(utf8) = std::str::from_utf8(bytes) {
        return utf8.to_string();
    }
    let (decoded, _, _) = encoding_rs::BIG5.decode(bytes);
    decoded.into_owned()
}

/// Locate `xelatex` executable either in PATH or common Windows MiKTeX / TeX Live directories.
pub fn find_xelatex_bin_dir() -> Option<PathBuf> {
    let exe_name = if cfg!(target_os = "windows") {
        "xelatex.exe"
    } else {
        "xelatex"
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
        if let Ok(entries) = std::fs::read_dir(r"C:\texlive") {
            for entry in entries.flatten() {
                candidates.push(entry.path().join(r"bin\windows"));
                candidates.push(entry.path().join(r"bin\win32"));
            }
        }
        for dir in candidates {
            if dir.join(exe_name).is_file() {
                return Some(dir);
            }
        }
    }

    None
}

/// Run `build.cmd` (if present on Windows) or `xelatex` in `root_dir`, returning a clean Chinese summary.
pub fn run_latex_build(root_dir: &Path, quick: bool) -> CompileOutcome {
    let Some(xelatex_dir) = find_xelatex_bin_dir() else {
        let msg = "本機尚未安裝 XeLaTeX（MiKTeX / TeX Live），無法在本機編譯 PDF；請安裝 MiKTeX 或執行 scripts/install-miktex.ps1。".to_string();
        return CompileOutcome {
            success: false,
            summary: msg.clone(),
            details: msg,
        };
    };

    let mut path_entries = vec![xelatex_dir];
    if let Some(existing) = std::env::var_os("PATH") {
        path_entries.extend(std::env::split_paths(&existing));
    }
    let joined_path = std::env::join_paths(path_entries).ok();

    let has_build_cmd = root_dir.join("build.cmd").exists();
    let mut cmd = if cfg!(target_os = "windows") && has_build_cmd {
        let mut c = Command::new("cmd");
        c.current_dir(root_dir).arg("/C").arg("build.cmd");
        if quick {
            c.arg("-Quick");
        }
        c
    } else {
        let mut c = Command::new("xelatex");
        c.current_dir(root_dir)
            .args(["-interaction=nonstopmode", "-halt-on-error", "main.tex"]);
        c
    };

    if let Some(p) = joined_path {
        cmd.env("PATH", p);
    }

    match cmd.output() {
        Ok(out) => {
            let stdout = decode_console_bytes(&out.stdout);
            let stderr = decode_console_bytes(&out.stderr);
            if out.status.success() {
                CompileOutcome {
                    success: true,
                    summary: "XeLaTeX 編譯成功：main.pdf 已更新".to_string(),
                    details: format!("{stdout}\n{stderr}"),
                }
            } else {
                let first_err = stderr
                    .lines()
                    .chain(stdout.lines())
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("請檢視 main.log")
                    .chars()
                    .take(120)
                    .collect::<String>();
                CompileOutcome {
                    success: false,
                    summary: format!("編譯失敗（{}）：{}", out.status, first_err),
                    details: format!("--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"),
                }
            }
        }
        Err(e) => {
            let msg = format!("無法啟動編譯器：{e}");
            CompileOutcome {
                success: false,
                summary: msg.clone(),
                details: msg,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_big5_and_utf8_cleanly() {
        assert_eq!(decode_console_bytes("中文測試".as_bytes()), "中文測試");
        let (big5_bytes, _, _) = encoding_rs::BIG5.encode("無法辨識 'xelatex'");
        assert_eq!(decode_console_bytes(&big5_bytes), "無法辨識 'xelatex'");
    }
}
