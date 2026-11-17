use crate::compile::run_latex_build;
use crate::state::{EditorSharedState, EditorSyncEvent};
use latex_ast::{
    Block, BlockKind, MathEnv, inlines_plain_text, parse_latex, serialize_block, serialize_latex,
};
use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerConfig},
    schemars, tool, tool_handler, tool_router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct TexWordMcpServer {
    pub state: Arc<RwLock<EditorSharedState>>,
    tool_router: ToolRouter<Self>,
}

impl TexWordMcpServer {
    pub fn new(state: Arc<RwLock<EditorSharedState>>) -> Self {
        Self {
            state,
            tool_router: Self::tool_router(),
        }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct OpenChapterArgs {
    #[schemars(description = "章節相對路徑或章名關鍵字，例如 chapters/ch01.tex 或 推進基礎")]
    pub chapter: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetBlockArgs {
    #[schemars(description = "區塊索引（從 0 開始）")]
    pub index: usize,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReplaceBlockArgs {
    #[schemars(description = "要替換的區塊索引（從 0 開始）")]
    pub index: usize,
    #[schemars(description = "新的 LaTeX 內容，將自動解析為 AST 並即時同步至 GPUI 編輯器")]
    pub latex: String,
    #[schemars(description = "是否立即存回 .tex 檔案（預設 true）")]
    pub save: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct InsertBlockArgs {
    #[schemars(description = "插入在哪個區塊索引之後；若省略或超過長度則附加至章節末尾")]
    pub after_index: Option<usize>,
    #[schemars(description = "要插入的 LaTeX 內容（可為段落、標題、公式或例題/重點/注意方框）")]
    pub latex: String,
    #[schemars(description = "是否立即存回 .tex 檔案（預設 true）")]
    pub save: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DeleteBlockArgs {
    #[schemars(description = "要刪除的區塊索引（從 0 開始）")]
    pub index: usize,
    #[schemars(description = "是否立即存回 .tex 檔案（預設 true）")]
    pub save: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct InsertEquationArgs {
    #[schemars(description = "插入在哪個區塊索引之後；省略則加到末尾")]
    pub after_index: Option<usize>,
    #[schemars(description = "LaTeX 數學式內容（不含 begin/end），例如 F = \\dot{m} v_e")]
    pub tex: String,
    #[schemars(description = "可選的公式標籤，例如 eq:thrust-demo")]
    pub label: Option<String>,
    #[schemars(description = "是否編號（預設 true）")]
    pub numbered: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchManuscriptArgs {
    #[schemars(description = "要搜尋的關鍵字（中英文皆可）")]
    pub query: String,
    #[schemars(description = "最多回傳筆數（預設 30）")]
    pub max_hits: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CheckTermsArgs {
    #[schemars(description = "是否檢查整本書稿（false 則只檢查目前開啟的章節）")]
    pub full_book: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CompilePdfArgs {
    #[schemars(description = "是否使用快速單次編譯（對應 build.cmd -Quick，預設 true）")]
    pub quick: Option<bool>,
}

#[derive(Serialize)]
struct BlockSummary {
    index: usize,
    id: String,
    kind: &'static str,
    preview: String,
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for TexWordMcpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "TeXWord GPUI 即時 LaTeX 書稿編輯器 MCP Server。可讀取、編輯與插入目前開啟的 .tex 章節區塊，畫面會即時同步更新，並支援術語檢查與 XeLaTeX 編譯。",
        )
    }
}

#[tool_router]
impl TexWordMcpServer {
    #[tool(description = "列出目前書稿專案（main.tex）包含的所有章節與附錄，以及當前開啟的章節。")]
    async fn list_chapters(&self) -> Result<String, String> {
        let guard = self.state.read().await;
        let Some(project) = &guard.project else {
            return Err("尚未開啟任何書稿專案".to_string());
        };
        let payload = serde_json::json!({
            "root_dir": project.root_dir,
            "active_chapter": guard.active_chapter,
            "chapters": project.chapters,
        });
        serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())
    }

    #[tool(description = "在 GPUI 編輯器中切換並開啟指定章節（依相對路徑如 chapters/ch01.tex 或章名關鍵字）。")]
    async fn open_chapter(
        &self,
        Parameters(OpenChapterArgs { chapter }): Parameters<OpenChapterArgs>,
    ) -> Result<String, String> {
        let mut guard = self.state.write().await;
        let opened = guard
            .open_chapter_path(&chapter)
            .map_err(|e| e.to_string())?;
        guard.push_log(format!("[MCP] open_chapter -> {opened}"));
        Ok(format!("已開啟章節：{opened}"))
    }

    #[tool(description = "取得目前開啟章節的結構化區塊列表（含索引、類型與文字摘要）。")]
    async fn get_active_document(&self) -> Result<String, String> {
        let guard = self.state.read().await;
        let summaries: Vec<BlockSummary> = guard
            .document
            .blocks
            .iter()
            .enumerate()
            .map(|(idx, b)| BlockSummary {
                index: idx,
                id: b.id.clone(),
                kind: block_kind_label(&b.kind),
                preview: block_preview(&b.kind),
            })
            .collect();
        let payload = serde_json::json!({
            "active_chapter": guard.active_chapter,
            "dirty": guard.dirty,
            "block_count": summaries.len(),
            "blocks": summaries,
        });
        serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())
    }

    #[tool(description = "讀取目前章節中指定索引區塊的完整 LaTeX 原始碼與 AST。")]
    async fn get_block(
        &self,
        Parameters(GetBlockArgs { index }): Parameters<GetBlockArgs>,
    ) -> Result<String, String> {
        let guard = self.state.read().await;
        let block = guard
            .document
            .blocks
            .get(index)
            .ok_or_else(|| format!("區塊索引 {index} 超出範圍"))?;
        let payload = serde_json::json!({
            "index": index,
            "id": block.id,
            "latex": serialize_block(block),
            "ast": block.kind,
        });
        serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())
    }

    #[tool(description = "替換指定索引的區塊內容（傳入新的 LaTeX），並即時同步 GPUI 編輯器畫面。")]
    async fn replace_block(
        &self,
        Parameters(ReplaceBlockArgs { index, latex, save }): Parameters<ReplaceBlockArgs>,
    ) -> Result<String, String> {
        let mut parsed = parse_latex(&latex);
        if parsed.blocks.is_empty() {
            return Err("傳入的 LaTeX 內容為空，無法解析出區塊".to_string());
        }
        let mut guard = self.state.write().await;
        if index >= guard.document.blocks.len() {
            return Err(format!("區塊索引 {index} 超出範圍"));
        }
        let inserted_count = parsed.blocks.len();
        guard
            .document
            .blocks
            .splice(index..=index, parsed.blocks.drain(..));
        guard.dirty = true;
        if save.unwrap_or(true) {
            guard.save_active_to_disk().map_err(|e| e.to_string())?;
        }
        let msg = format!("已替換區塊 #{index}（新產生 {inserted_count} 個區塊）");
        guard.push_log(format!("[MCP] {msg}"));
        let _ = guard.events_tx.send(EditorSyncEvent::DocumentUpdated {
            source: "mcp:replace_block".to_string(),
            summary: msg.clone(),
        });
        Ok(msg)
    }

    #[tool(description = "在指定索引之後插入新的 LaTeX 區塊（段落、標題、公式、方框皆可），並即時同步畫面。")]
    async fn insert_block(
        &self,
        Parameters(InsertBlockArgs {
            after_index,
            latex,
            save,
        }): Parameters<InsertBlockArgs>,
    ) -> Result<String, String> {
        let parsed = parse_latex(&latex);
        if parsed.blocks.is_empty() {
            return Err("無法從傳入內容解析出有效區塊".to_string());
        }
        let mut guard = self.state.write().await;
        let insert_pos = match after_index {
            Some(idx) if idx + 1 <= guard.document.blocks.len() => idx + 1,
            _ => guard.document.blocks.len(),
        };
        let count = parsed.blocks.len();
        for (offset, b) in parsed.blocks.into_iter().enumerate() {
            guard.document.blocks.insert(insert_pos + offset, b);
        }
        guard.dirty = true;
        if save.unwrap_or(true) {
            guard.save_active_to_disk().map_err(|e| e.to_string())?;
        }
        let msg = format!("已於位置 #{insert_pos} 插入 {count} 個區塊");
        guard.push_log(format!("[MCP] {msg}"));
        let _ = guard.events_tx.send(EditorSyncEvent::DocumentUpdated {
            source: "mcp:insert_block".to_string(),
            summary: msg.clone(),
        });
        Ok(msg)
    }

    #[tool(description = "刪除目前章節中指定索引的區塊，並即時同步畫面。")]
    async fn delete_block(
        &self,
        Parameters(DeleteBlockArgs { index, save }): Parameters<DeleteBlockArgs>,
    ) -> Result<String, String> {
        let mut guard = self.state.write().await;
        if index >= guard.document.blocks.len() {
            return Err(format!("區塊索引 {index} 超出範圍"));
        }
        guard.document.blocks.remove(index);
        guard.dirty = true;
        if save.unwrap_or(true) {
            guard.save_active_to_disk().map_err(|e| e.to_string())?;
        }
        let msg = format!("已刪除區塊 #{index}");
        guard.push_log(format!("[MCP] {msg}"));
        let _ = guard.events_tx.send(EditorSyncEvent::DocumentUpdated {
            source: "mcp:delete_block".to_string(),
            summary: msg.clone(),
        });
        Ok(msg)
    }

    #[tool(description = "快速在章節中插入一個獨立數學公式區塊（equation / equation*），即時渲染並寫回 .tex。")]
    async fn insert_equation(
        &self,
        Parameters(InsertEquationArgs {
            after_index,
            tex,
            label,
            numbered,
        }): Parameters<InsertEquationArgs>,
    ) -> Result<String, String> {
        let env = if numbered.unwrap_or(true) {
            MathEnv::Equation
        } else {
            MathEnv::EquationStar
        };
        let block = Block::new(BlockKind::MathBlock { env, tex, label });
        let mut guard = self.state.write().await;
        let insert_pos = match after_index {
            Some(idx) if idx + 1 <= guard.document.blocks.len() => idx + 1,
            _ => guard.document.blocks.len(),
        };
        guard.document.blocks.insert(insert_pos, block);
        guard.dirty = true;
        guard.save_active_to_disk().map_err(|e| e.to_string())?;
        let msg = format!("已於位置 #{insert_pos} 插入公式區塊");
        guard.push_log(format!("[MCP] {msg}"));
        let _ = guard.events_tx.send(EditorSyncEvent::DocumentUpdated {
            source: "mcp:insert_equation".to_string(),
            summary: msg.clone(),
        });
        Ok(msg)
    }

    #[tool(description = "在整份書稿的所有 .tex 章節中搜尋關鍵字。")]
    async fn search_manuscript(
        &self,
        Parameters(SearchManuscriptArgs { query, max_hits }): Parameters<SearchManuscriptArgs>,
    ) -> Result<String, String> {
        let guard = self.state.read().await;
        let Some(project) = &guard.project else {
            return Err("尚未開啟任何書稿專案".to_string());
        };
        let hits = project.search(&query, max_hits.unwrap_or(30));
        let results: Vec<_> = hits
            .into_iter()
            .map(|(file, line, text)| {
                serde_json::json!({
                    "file": file,
                    "line": line,
                    "text": text,
                })
            })
            .collect();
        serde_json::to_string_pretty(&results).map_err(|e| e.to_string())
    }

    #[tool(description = "依 terms/terms.csv 檢查避免用詞（如大陸譯名）、\\todo 與 \\needcite 標記。")]
    async fn check_terms(
        &self,
        Parameters(CheckTermsArgs { full_book }): Parameters<CheckTermsArgs>,
    ) -> Result<String, String> {
        let guard = self.state.read().await;
        let Some(project) = &guard.project else {
            return Err("尚未開啟任何書稿專案".to_string());
        };
        let mut issues = Vec::new();
        if full_book.unwrap_or(false) {
            for ch in &project.chapters {
                if let Ok(raw) = std::fs::read_to_string(&ch.full_path) {
                    issues.extend(project.check_text_issues(&ch.relative_path, &raw));
                }
            }
        } else {
            let label = guard
                .active_chapter
                .clone()
                .unwrap_or_else(|| "active.tex".to_string());
            let raw = serialize_latex(&guard.document);
            issues = project.check_text_issues(&label, &raw);
        }
        let payload = serde_json::json!({
            "issue_count": issues.len(),
            "issues": issues,
        });
        serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())
    }

    #[tool(description = "在書稿根目錄執行 XeLaTeX 編譯（Windows 呼叫 build.cmd -Quick 或 xelatex）。")]
    async fn compile_pdf(
        &self,
        Parameters(CompilePdfArgs { quick }): Parameters<CompilePdfArgs>,
    ) -> Result<String, String> {
        let root_dir = {
            let mut guard = self.state.write().await;
            if guard.dirty {
                guard.save_active_to_disk().map_err(|e| e.to_string())?;
            }
            let Some(project) = &guard.project else {
                return Err("尚未開啟書稿專案".to_string());
            };
            let root = project.root_dir.clone();
            guard.push_log("[MCP] 開始執行 LaTeX 編譯...");
            root
        };

        let quick_flag = quick.unwrap_or(true);
        let outcome = tokio::task::spawn_blocking(move || run_latex_build(&root_dir, quick_flag))
            .await
            .map_err(|e| e.to_string())?;

        let mut guard = self.state.write().await;
        guard.push_log(format!("[MCP] {}", outcome.summary));
        if outcome.success {
            Ok(format!("{}\n{}", outcome.summary, outcome.details))
        } else {
            Err(format!("{}\n{}", outcome.summary, outcome.details))
        }
    }
}

fn block_kind_label(kind: &BlockKind) -> &'static str {
    match kind {
        BlockKind::Heading { .. } => "heading",
        BlockKind::Paragraph { .. } => "paragraph",
        BlockKind::MathBlock { .. } => "math_block",
        BlockKind::Figure { .. } => "figure",
        BlockKind::Callout { .. } => "callout",
        BlockKind::List { .. } => "list",
        BlockKind::Comment { .. } => "comment",
        BlockKind::RawLatex { .. } => "raw_latex",
    }
}

fn block_preview(kind: &BlockKind) -> String {
    let raw = match kind {
        BlockKind::Heading { title, .. } => title.clone(),
        BlockKind::Paragraph { inlines } => inlines_plain_text(inlines),
        BlockKind::MathBlock { tex, .. } => format!("$$ {} $$", tex.replace('\n', " ")),
        BlockKind::Figure {
            caption,
            path,
            has_tikzpicture,
            ..
        } => {
            if *has_tikzpicture {
                format!("[TikZ 圖] {}", caption.as_deref().unwrap_or(""))
            } else {
                format!(
                    "[圖 {}] {}",
                    path.as_deref().unwrap_or(""),
                    caption.as_deref().unwrap_or("")
                )
            }
        }
        BlockKind::Callout { kind, title, .. } => {
            format!("[{}] {}", kind.display_label(), title.as_deref().unwrap_or(""))
        }
        BlockKind::List { items, .. } => format!("清單（{} 項）", items.len()),
        BlockKind::Comment { lines } => lines.first().cloned().unwrap_or_default(),
        BlockKind::RawLatex { env_name, raw } => format!(
            "[Raw: {}] {}",
            env_name.as_deref().unwrap_or("tex"),
            raw.lines().next().unwrap_or("")
        ),
    };
    raw.chars().take(100).collect()
}
