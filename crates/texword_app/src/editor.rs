use crate::{
    math_view::render_math_tex,
    metadata::BookMetadata,
    pdf_preview::{estimate_chapter_pdf_pages, get_pdf_page_count, render_pdf_page_png},
    theme::BookPalette,
};
use gpui::{
    AnyElement, AppContext as _, Context, Entity, FocusHandle, Focusable as _, FontStyle,
    FontWeight, HighlightStyle, Image, ImageFormat, InteractiveElement, IntoElement, ListAlignment,
    ListState, ParentElement, PathPromptOptions, Render, SharedString, StatefulInteractiveElement,
    Styled, StyledImage as _, StyledText, Subscription, Window, actions, div, img, list,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Sizable as _,
    button::{Button, ButtonVariants as _},
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
    scroll::{ScrollableElement as _, Scrollbar},
};
use latex_ast::{
    Block, BlockKind, CalloutKind, ChapterEntry, Document, HeadingLevel, Inline, TermIssue,
    parse_latex, serialize_block, serialize_latex,
};
use math_preview::{format_chem_unicode, math_to_plain_text, parse_math};
use mcp_server::{EditorSharedState, EditorSyncEvent, run_latex_build};
use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};

actions!(texword, [SaveFile, ToggleSearch]);

#[derive(Clone, Copy, PartialEq, Eq)]
enum RightTab {
    Preview,
    Terms,
    Meta,
    Mcp,
}

#[derive(Clone)]
struct SearchHit {
    chapter_rel: String,
    block_idx: Option<usize>,
    line: usize,
    snippet: String,
}

pub struct TexWordEditor {
    shared: Arc<RwLock<EditorSharedState>>,
    mcp_endpoint: String,
    chapters: Vec<ChapterEntry>,
    active_chapter: Option<String>,
    document: Document,
    dirty: bool,
    doc_list_state: ListState,
    selected_block: Option<usize>,
    editing_block: Option<usize>,
    edit_textarea: Entity<TextareaState>,
    live_preview_blocks: Vec<Block>,
    expanded_raw: std::collections::HashSet<String>,
    show_search: bool,
    search_input: Entity<InputState>,
    search_hits: Vec<SearchHit>,
    show_right_panel: bool,
    right_tab: RightTab,
    term_issues: Vec<TermIssue>,
    mcp_logs: Vec<String>,
    pdf_path: Option<PathBuf>,
    pdf_page: usize,
    pdf_total_pages: usize,
    pdf_dpi: u32,
    pdf_page_input: Entity<InputState>,
    pdf_image: Option<Arc<Image>>,
    pdf_loading: bool,
    pdf_error: Option<String>,
    toc_pages: Vec<(String, usize)>,
    figure_thumbs: HashMap<String, Arc<Image>>,
    meta_author_input: Entity<InputState>,
    meta_publisher_input: Entity<InputState>,
    meta_date_input: Entity<InputState>,
    focus_handle: FocusHandle,
    status_message: String,
    _sync_task: gpui::Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl TexWordEditor {
    pub fn new(
        shared: Arc<RwLock<EditorSharedState>>,
        mcp_endpoint: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let edit_textarea = cx.new(|cx| TextareaState::new(window, cx));
        let edit_sub = cx.subscribe(&edit_textarea, |this, state, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) && this.editing_block.is_some() {
                let text = state.read(cx).value().to_string();
                this.live_preview_blocks = parse_latex(&text).blocks;
                cx.notify();
            }
        });

        let pdf_page_input = cx.new(|cx| InputState::new(window, cx));
        let page_sub = cx.subscribe_in(&pdf_page_input, window, |this, _, ev: &InputEvent, window, cx| {
            if matches!(ev, InputEvent::PressEnter { .. }) {
                this.jump_pdf_from_input(window, cx);
            }
        });

        let search_input = cx.new(|cx| InputState::new(window, cx));
        let search_sub = cx.subscribe_in(&search_input, window, |this, _, ev: &InputEvent, _window, cx| {
            if matches!(ev, InputEvent::PressEnter { .. }) {
                this.run_search(false, cx);
            }
        });

        let meta_author_input = cx.new(|cx| InputState::new(window, cx));
        let meta_publisher_input = cx.new(|cx| InputState::new(window, cx));
        let meta_date_input = cx.new(|cx| InputState::new(window, cx));

        let (chapters, active_chapter, document, dirty, mcp_logs, root_dir, mut rx) = {
            let guard = shared.blocking_read();
            let chapters = guard
                .project
                .as_ref()
                .map(|p| p.chapters.clone())
                .unwrap_or_default();
            let root_dir = guard.project.as_ref().map(|p| p.root_dir.clone());
            (
                chapters,
                guard.active_chapter.clone(),
                guard.document.clone(),
                guard.dirty,
                guard.mcp_logs.clone(),
                root_dir,
                guard.events_tx.subscribe(),
            )
        };

        let pdf_path = root_dir
            .as_ref()
            .map(|r| r.join("main.pdf"))
            .filter(|p| p.exists());
        let toc_pages = root_dir
            .as_ref()
            .map(|r| estimate_chapter_pdf_pages(r))
            .unwrap_or_default();

        let doc_list_state =
            ListState::new(document.blocks.len(), ListAlignment::Top, px(480.0));

        let shared_for_task = shared.clone();
        let sync_task = cx.spawn(async move |this, cx| {
            loop {
                match rx.recv().await {
                    Ok(ev) => {
                        let reset_scroll = matches!(ev, EditorSyncEvent::ChapterOpened { .. });
                        let summary = match &ev {
                            EditorSyncEvent::ChapterOpened { path } => {
                                format!("已切換章節：{path}")
                            }
                            EditorSyncEvent::DocumentUpdated { summary, .. } => summary.clone(),
                            EditorSyncEvent::LogMessage { message, .. } => message.clone(),
                        };
                        let _ = this.update(cx, |editor, cx| {
                            editor.pull_from_shared(&shared_for_task, reset_scroll);
                            editor.status_message = summary;
                            cx.notify();
                        });
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        let _ = this.update(cx, |editor, cx| {
                            editor.pull_from_shared(&shared_for_task, false);
                            cx.notify();
                        });
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        let mut editor = Self {
            shared,
            mcp_endpoint,
            chapters,
            active_chapter,
            document,
            dirty,
            doc_list_state,
            selected_block: None,
            editing_block: None,
            edit_textarea,
            live_preview_blocks: Vec::new(),
            expanded_raw: std::collections::HashSet::new(),
            show_search: false,
            search_input,
            search_hits: Vec::new(),
            show_right_panel: true,
            right_tab: RightTab::Preview,
            term_issues: Vec::new(),
            mcp_logs,
            pdf_path,
            pdf_page: 1,
            pdf_total_pages: 1,
            pdf_dpi: 130,
            pdf_page_input,
            pdf_image: None,
            pdf_loading: false,
            pdf_error: None,
            toc_pages,
            figure_thumbs: HashMap::new(),
            meta_author_input,
            meta_publisher_input,
            meta_date_input,
            focus_handle: cx.focus_handle(),
            status_message:
                "就緒：Ctrl+S 存檔、Ctrl+F 搜尋、右側可預覽 PDF 或設定封面與作者。"
                    .to_string(),
            _sync_task: sync_task,
            _subscriptions: vec![edit_sub, page_sub, search_sub],
        };
        editor.refresh_term_issues();
        editor.sync_meta_inputs(window, cx);
        editor.jump_pdf_to_current_chapter();
        editor.sync_pdf_page_input(window, cx);
        editor.refresh_pdf_preview(true, cx);
        editor.load_figure_thumbnails(cx);
        editor
    }

    fn pull_from_shared(&mut self, shared: &Arc<RwLock<EditorSharedState>>, reset_list: bool) {
        let old_len = self.document.blocks.len();
        let guard = shared.blocking_read();
        if let Some(p) = &guard.project {
            self.chapters = p.chapters.clone();
        }
        self.active_chapter = guard.active_chapter.clone();
        self.document = guard.document.clone();
        self.dirty = guard.dirty;
        self.mcp_logs = guard.mcp_logs.clone();
        let new_len = self.document.blocks.len();
        drop(guard);

        if reset_list || old_len != new_len {
            self.doc_list_state.reset(new_len);
        } else if new_len > 0 {
            self.doc_list_state.splice(0..new_len, new_len);
        }
        self.refresh_term_issues();
    }

    fn push_document_to_shared(&mut self) {
        let mut guard = self.shared.blocking_write();
        guard.document = self.document.clone();
        guard.dirty = self.dirty;
    }

    fn refresh_term_issues(&mut self) {
        let guard = self.shared.blocking_read();
        if let Some(project) = &guard.project {
            let label = self
                .active_chapter
                .clone()
                .unwrap_or_else(|| "active.tex".to_string());
            let raw = serialize_latex(&self.document);
            self.term_issues = project.check_text_issues(&label, &raw);
        }
    }

    fn open_chapter(&mut self, rel_path: &str, window: &mut Window, cx: &mut Context<Self>) {
        let mut guard = self.shared.blocking_write();
        match guard.open_chapter_path(rel_path) {
            Ok(opened) => {
                self.active_chapter = Some(opened.clone());
                self.document = guard.document.clone();
                self.dirty = false;
                self.editing_block = None;
                self.selected_block = None;
                self.doc_list_state.reset(self.document.blocks.len());
                self.status_message = format!("已開啟：{opened}");
            }
            Err(e) => {
                self.status_message = format!("開啟失敗：{e}");
            }
        }
        drop(guard);
        self.refresh_term_issues();
        self.jump_pdf_to_current_chapter();
        self.sync_pdf_page_input(window, cx);
        self.refresh_pdf_preview(false, cx);
        self.load_figure_thumbnails(cx);
        cx.notify();
    }

    fn load_figure_thumbnails(&mut self, cx: &mut Context<Self>) {
        let root_dir = {
            let guard = self.shared.blocking_read();
            guard.project.as_ref().map(|p| p.root_dir.clone())
        };
        let Some(root) = root_dir else {
            return;
        };
        let mut fig_paths = Vec::new();
        for b in &self.document.blocks {
            if let BlockKind::Figure { path: Some(p), .. } = &b.kind {
                if !self.figure_thumbs.contains_key(p) {
                    fig_paths.push(p.clone());
                }
            }
        }
        if fig_paths.is_empty() {
            return;
        }
        cx.spawn(async move |this, cx| {
            let rendered = cx
                .background_executor()
                .spawn(async move {
                    let mut out = Vec::new();
                    for rel in fig_paths {
                        let candidates = [
                            root.join("figures/out").join(format!("{rel}.pdf")),
                            root.join("figures/out").join(&rel),
                            root.join(&rel),
                        ];
                        for cand in candidates {
                            if !cand.is_file() {
                                continue;
                            }
                            let ext = cand
                                .extension()
                                .and_then(|e| e.to_str())
                                .unwrap_or("")
                                .to_ascii_lowercase();
                            if ext == "pdf" {
                                if let Ok(bytes) = render_pdf_page_png(&cand, 1, 110) {
                                    out.push((rel, ImageFormat::Png, bytes));
                                    break;
                                }
                            } else if let Ok(bytes) = std::fs::read(&cand) {
                                let fmt = match ext.as_str() {
                                    "jpg" | "jpeg" => ImageFormat::Jpeg,
                                    "webp" => ImageFormat::Webp,
                                    _ => ImageFormat::Png,
                                };
                                out.push((rel, fmt, bytes));
                                break;
                            }
                        }
                    }
                    out
                })
                .await;

            if !rendered.is_empty() {
                let _ = this.update(cx, |editor, cx| {
                    for (key, fmt, bytes) in rendered {
                        editor
                            .figure_thumbs
                            .insert(key, Arc::new(Image::from_bytes(fmt, bytes)));
                    }
                    let n = editor.document.blocks.len();
                    if n > 0 {
                        editor.doc_list_state.splice(0..n, n);
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn jump_to_issue(&mut self, issue: &TermIssue, cx: &mut Context<Self>) {
        let needle = issue.matched.trim();
        let ctx_needle: String = issue.context.chars().take(24).collect();
        let target_idx = self.document.blocks.iter().position(|b| {
            let raw = serialize_block(b);
            (!needle.is_empty() && raw.contains(needle))
                || (!ctx_needle.is_empty() && raw.contains(&ctx_needle))
        });

        if let Some(idx) = target_idx {
            self.selected_block = Some(idx);
            self.editing_block = None;
            self.doc_list_state.splice(idx..idx + 1, 1);
            self.doc_list_state.scroll_to_reveal_item(idx);
            self.status_message = format!(
                "已定位至區塊 #{}（{} 第 {} 行）",
                idx + 1,
                issue.kind,
                issue.line
            );
            cx.notify();
        } else {
            self.status_message = format!("無法定位至第 {} 行：{}", issue.line, issue.matched);
            cx.notify();
        }
    }

    fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_search = !self.show_search;
        if self.show_search {
            let focus = self.search_input.read(cx).focus_handle(cx);
            window.focus(&focus, cx);
        } else {
            self.search_hits.clear();
        }
        cx.notify();
    }

    fn run_search(&mut self, full_book: bool, cx: &mut Context<Self>) {
        let query = self.search_input.read(cx).value().trim().to_string();
        if query.is_empty() {
            self.search_hits.clear();
            cx.notify();
            return;
        }
        let q_lower = query.to_lowercase();
        let mut hits = Vec::new();
        if full_book {
            let guard = self.shared.blocking_read();
            if let Some(project) = &guard.project {
                for (file, line, snippet) in project.search(&query, 30) {
                    hits.push(SearchHit {
                        chapter_rel: file,
                        block_idx: None,
                        line,
                        snippet,
                    });
                }
            }
        } else {
            let active = self
                .active_chapter
                .clone()
                .unwrap_or_else(|| "active.tex".to_string());
            for (idx, b) in self.document.blocks.iter().enumerate() {
                let raw = serialize_block(b);
                if raw.to_lowercase().contains(&q_lower) {
                    let snippet = raw.lines().next().unwrap_or("").chars().take(60).collect();
                    hits.push(SearchHit {
                        chapter_rel: active.clone(),
                        block_idx: Some(idx),
                        line: idx + 1,
                        snippet,
                    });
                }
            }
        }
        self.status_message = format!("搜尋「{query}」找到 {} 筆結果", hits.len());
        self.search_hits = hits;
        cx.notify();
    }

    fn jump_to_search_hit(&mut self, hit: &SearchHit, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_chapter.as_deref() != Some(hit.chapter_rel.as_str()) {
            self.open_chapter(&hit.chapter_rel, window, cx);
        }
        let target_idx = hit.block_idx.or_else(|| {
            let needle: String = hit.snippet.chars().take(20).collect();
            self.document
                .blocks
                .iter()
                .position(|b| serialize_block(b).contains(&needle))
        });
        if let Some(idx) = target_idx {
            self.selected_block = Some(idx);
            self.editing_block = None;
            self.doc_list_state.splice(idx..idx + 1, 1);
            self.doc_list_state.scroll_to_reveal_item(idx);
            self.status_message = format!("已跳轉至 {} 區塊 #{}", hit.chapter_rel, idx + 1);
        }
        cx.notify();
    }

    fn open_project_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("選擇 LaTeX 書稿專案資料夾".into()),
        });
        let entity = cx.entity();
        window
            .spawn(cx, async move |cx| {
                if let Ok(Ok(Some(paths))) = rx.await
                    && let Some(dir) = paths.into_iter().next()
                {
                    let _ = cx.update(|window, cx| {
                        let _ = entity.update(cx, |editor, cx| {
                            editor.load_project_dir(&dir, window, cx);
                        });
                    });
                }
            })
            .detach();
    }

    fn load_project_dir(&mut self, dir: &PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let mut guard = self.shared.blocking_write();
        match guard.open_project(dir) {
            Ok(()) => {
                self.chapters = guard
                    .project
                    .as_ref()
                    .map(|p| p.chapters.clone())
                    .unwrap_or_default();
                self.active_chapter = guard.active_chapter.clone();
                self.document = guard.document.clone();
                self.dirty = false;
                self.editing_block = None;
                self.selected_block = None;
                self.figure_thumbs.clear();
                let pdf = dir.join("main.pdf");
                self.pdf_path = if pdf.exists() { Some(pdf) } else { None };
                self.toc_pages = estimate_chapter_pdf_pages(dir);
                self.doc_list_state.reset(self.document.blocks.len());
                self.status_message = format!("已開啟專案：{}", dir.display());
            }
            Err(e) => {
                self.status_message = format!("開啟專案失敗：{e}");
            }
        }
        drop(guard);
        self.refresh_term_issues();
        self.sync_meta_inputs(window, cx);
        self.jump_pdf_to_current_chapter();
        self.sync_pdf_page_input(window, cx);
        self.refresh_pdf_preview(true, cx);
        self.load_figure_thumbnails(cx);
        cx.notify();
    }

    fn sync_meta_inputs(&self, window: &mut Window, cx: &mut Context<Self>) {
        let root_opt = {
            let guard = self.shared.blocking_read();
            guard.project.as_ref().map(|p| p.root_dir.clone())
        };
        let Some(root) = root_opt else {
            return;
        };
        let meta = BookMetadata::load_from_project(&root);
        self.meta_author_input.update(cx, |s, cx| {
            s.set_value(meta.author, window, cx);
        });
        self.meta_publisher_input.update(cx, |s, cx| {
            s.set_value(meta.publisher, window, cx);
        });
        self.meta_date_input.update(cx, |s, cx| {
            s.set_value(meta.date_ym, window, cx);
        });
    }

    fn apply_metadata_to_project(&mut self, cx: &mut Context<Self>) {
        let root_opt = {
            let guard = self.shared.blocking_read();
            guard.project.as_ref().map(|p| p.root_dir.clone())
        };
        let Some(root) = root_opt else {
            return;
        };
        let meta = BookMetadata {
            author: self.meta_author_input.read(cx).value().to_string(),
            publisher: self.meta_publisher_input.read(cx).value().to_string(),
            date_ym: self.meta_date_input.read(cx).value().to_string(),
        };
        match meta.save_to_project(&root) {
            Ok(files) => {
                {
                    let mut guard = self.shared.blocking_write();
                    let _ = guard.reload_from_disk();
                    self.document = guard.document.clone();
                    self.dirty = false;
                    guard.push_log(format!("[Meta] 已更新 {}", files.join("、")));
                }
                self.doc_list_state.reset(self.document.blocks.len());
                self.refresh_term_issues();
                self.status_message = format!(
                    "已寫入 {}（可按「XeLaTeX 快速編譯」更新 PDF）",
                    if files.is_empty() {
                        "無變更".to_string()
                    } else {
                        files.join("、")
                    }
                );
            }
            Err(e) => {
                self.status_message = format!("更新封面／書目失敗：{e}");
            }
        }
        cx.notify();
    }

    fn jump_pdf_to_current_chapter(&mut self) {
        let Some(active) = self.active_chapter.as_deref() else {
            return;
        };
        let title = self
            .chapters
            .iter()
            .find(|c| c.relative_path == active)
            .map(|c| c.title.as_str())
            .unwrap_or("");
        if title.is_empty() {
            return;
        }
        if let Some((_, page)) = self.toc_pages.iter().find(|(t, _)| t.contains(title)) {
            self.pdf_page = (*page).max(1);
        }
    }

    fn sync_pdf_page_input(&self, window: &mut Window, cx: &mut Context<Self>) {
        let p = self.pdf_page.to_string();
        self.pdf_page_input.update(cx, |s, cx| {
            s.set_value(p, window, cx);
        });
    }

    fn jump_pdf_from_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let raw = self.pdf_page_input.read(cx).value().to_string();
        if let Ok(n) = raw.trim().parse::<usize>() {
            self.set_pdf_page(n, Some(window), cx);
        } else {
            self.sync_pdf_page_input(window, cx);
            self.status_message = format!("請輸入有效頁碼（1～{}）", self.pdf_total_pages.max(1));
            cx.notify();
        }
    }

    fn set_pdf_page(
        &mut self,
        page: usize,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        let total = self.pdf_total_pages.max(1);
        self.pdf_page = page.clamp(1, total);
        if let Some(w) = window {
            self.sync_pdf_page_input(w, cx);
        }
        self.refresh_pdf_preview(false, cx);
    }

    fn refresh_pdf_preview(&mut self, recount_pages: bool, cx: &mut Context<Self>) {
        let Some(pdf_path) = self.pdf_path.clone() else {
            self.pdf_error = Some("專案目錄下尚未產生 main.pdf（可先按「XeLaTeX 快速編譯」生成）".to_string());
            self.pdf_image = None;
            cx.notify();
            return;
        };
        if !pdf_path.exists() {
            self.pdf_error = Some("找不到 main.pdf".to_string());
            self.pdf_image = None;
            cx.notify();
            return;
        }

        let target_page = self.pdf_page.max(1);
        let dpi = self.pdf_dpi;
        self.pdf_loading = true;
        self.pdf_error = None;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let res = cx
                .background_executor()
                .spawn(async move {
                    let total = if recount_pages {
                        get_pdf_page_count(&pdf_path).ok()
                    } else {
                        None
                    };
                    let page = match total {
                        Some(t) => target_page.clamp(1, t),
                        None => target_page,
                    };
                    let png = render_pdf_page_png(&pdf_path, page, dpi);
                    (total, page, png)
                })
                .await;

            let _ = this.update(cx, |editor, cx| {
                editor.pdf_loading = false;
                if let Some(t) = res.0 {
                    editor.pdf_total_pages = t;
                }
                editor.pdf_page = res.1;
                match res.2 {
                    Ok(bytes) => {
                        editor.pdf_image =
                            Some(Arc::new(Image::from_bytes(ImageFormat::Png, bytes)));
                        editor.pdf_error = None;
                    }
                    Err(e) => {
                        editor.pdf_error = Some(e);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn save_active_file(&mut self, cx: &mut Context<Self>) {
        self.push_document_to_shared();
        let mut guard = self.shared.blocking_write();
        match guard.save_active_to_disk() {
            Ok(path) => {
                self.dirty = false;
                self.status_message = format!("已儲存：{}", path.display());
                guard.push_log(format!("[UI] 已儲存 {}", path.display()));
            }
            Err(e) => {
                self.status_message = format!("儲存失敗：{e}");
            }
        }
        drop(guard);
        self.refresh_term_issues();
        cx.notify();
    }

    fn start_editing_block(
        &mut self,
        idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(block) = self.document.blocks.get(idx) {
            let raw = serialize_block(block);
            self.selected_block = Some(idx);
            self.editing_block = Some(idx);
            self.live_preview_blocks = vec![block.clone()];
            self.edit_textarea.update(cx, |state, cx| {
                state.set_value(raw, window, cx);
            });
            self.doc_list_state.splice(idx..idx + 1, 1);
            self.doc_list_state.scroll_to_reveal_item(idx);
            cx.notify();
        }
    }

    fn apply_editing_block(&mut self, cx: &mut Context<Self>) {
        let Some(idx) = self.editing_block else {
            return;
        };
        let new_tex = self.edit_textarea.read(cx).value().to_string();
        let mut parsed = parse_latex(&new_tex);
        if parsed.blocks.is_empty() {
            self.status_message = "內容不可為空（若要刪除請點刪除按鈕）".to_string();
            cx.notify();
            return;
        }
        if idx < self.document.blocks.len() {
            let new_count = parsed.blocks.len();
            self.document
                .blocks
                .splice(idx..=idx, parsed.blocks.drain(..));
            self.dirty = true;
            self.editing_block = None;
            self.doc_list_state.splice(idx..idx + 1, new_count);
            self.push_document_to_shared();
            self.refresh_term_issues();
            self.status_message =
                format!("已更新區塊 #{}（可按「儲存 .tex」寫回檔案）", idx + 1);
        }
        cx.notify();
    }

    fn delete_selected_block(&mut self, cx: &mut Context<Self>) {
        let Some(idx) = self.editing_block.or(self.selected_block) else {
            return;
        };
        if idx < self.document.blocks.len() {
            self.document.blocks.remove(idx);
            self.dirty = true;
            self.editing_block = None;
            self.selected_block = None;
            self.doc_list_state.splice(idx..idx + 1, 0);
            self.push_document_to_shared();
            self.refresh_term_issues();
            self.status_message = format!("已刪除區塊 #{}", idx + 1);
        }
        cx.notify();
    }

    fn insert_template_block(
        &mut self,
        latex_snippet: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let parsed = parse_latex(latex_snippet);
        if parsed.blocks.is_empty() {
            return;
        }
        let insert_pos = match self.selected_block {
            Some(idx) if idx + 1 <= self.document.blocks.len() => idx + 1,
            _ => self.document.blocks.len(),
        };
        let count = parsed.blocks.len();
        for (offset, b) in parsed.blocks.into_iter().enumerate() {
            self.document.blocks.insert(insert_pos + offset, b);
        }
        self.dirty = true;
        self.doc_list_state.splice(insert_pos..insert_pos, count);
        self.push_document_to_shared();
        self.start_editing_block(insert_pos, window, cx);
    }

    fn append_inline_snippet(&mut self, snippet: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing_block.is_some() {
            let current = self.edit_textarea.read(cx).value().to_string();
            let updated = if current.is_empty() {
                snippet.to_string()
            } else {
                format!("{current} {snippet}")
            };
            self.edit_textarea.update(cx, |s, cx| {
                s.set_value(updated, window, cx);
            });
            cx.notify();
        } else {
            self.insert_template_block(snippet, window, cx);
        }
    }

    fn trigger_compile_quick(&mut self, cx: &mut Context<Self>) {
        self.save_active_file(cx);
        let root_opt: Option<PathBuf> = {
            let guard = self.shared.blocking_read();
            guard.project.as_ref().map(|p| p.root_dir.clone())
        };
        let Some(root) = root_opt else {
            self.status_message = "尚未開啟書稿資料夾".to_string();
            cx.notify();
            return;
        };

        self.status_message = "正在執行 XeLaTeX 快速編譯...".to_string();
        cx.notify();

        let shared = self.shared.clone();
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { run_latex_build(&root, true) })
                .await;

            let ok = outcome.success;
            let summary = outcome.summary;

            {
                let mut guard = shared.write().await;
                guard.push_log(format!("[Build] {summary}"));
            }
            let _ = this.update(cx, |editor, cx| {
                editor.pull_from_shared(&shared, false);
                editor.status_message = summary;
                if ok {
                    let guard = shared.blocking_read();
                    if let Some(p) = &guard.project {
                        editor.pdf_path = Some(p.root_dir.join("main.pdf"));
                        editor.toc_pages = estimate_chapter_pdf_pages(&p.root_dir);
                    }
                    drop(guard);
                    editor.right_tab = RightTab::Preview;
                    editor.show_right_panel = true;
                    editor.refresh_pdf_preview(true, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn render_ribbon(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w_full()
            .bg(BookPalette::ribbon_bg())
            .border_b_1()
            .border_color(BookPalette::border())
            .px(px(12.0))
            .py(px(8.0))
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(8.0))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .flex_wrap()
                    .gap(px(6.0))
                    .child(
                        Button::new("btn-open-proj")
                            .secondary()
                            .label("開啟專案")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_project_dialog(window, cx);
                            })),
                    )
                    .child(
                        Button::new("btn-save")
                            .primary()
                            .label(if self.dirty { "儲存 .tex *" } else { "儲存 .tex" })
                            .on_click(cx.listener(|this, _, _, cx| this.save_active_file(cx))),
                    )
                    .child(
                        Button::new("btn-search")
                            .secondary()
                            .label("搜尋 (Ctrl+F)")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_search(window, cx);
                            })),
                    )
                    .child(
                        Button::new("btn-add-para")
                            .secondary()
                            .label("+ 段落")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.insert_template_block("請在此輸入段落文字。", window, cx);
                            })),
                    )
                    .child(
                        Button::new("btn-add-sec")
                            .secondary()
                            .label("+ 小節")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.insert_template_block(
                                    "\\section{新小節標題}\\label{sec:new}",
                                    window,
                                    cx,
                                );
                            })),
                    )
                    .child(
                        Button::new("btn-add-eq")
                            .secondary()
                            .label("+ 公式")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.insert_template_block(
                                    "\\begin{equation}\n  F = \\dot{m}\\,v_e + (p_e - p_a)\\,A_e\n  \\label{eq:new}\n\\end{equation}",
                                    window,
                                    cx,
                                );
                            })),
                    )
                    .child(
                        Button::new("btn-add-example")
                            .secondary()
                            .label("+ 例題")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.insert_template_block(
                                    "\\begin{example}[例題標題]\n在這裡寫入計算步驟，單位使用 \\SI{2.0}{kg/s}。\n\\end{example}",
                                    window,
                                    cx,
                                );
                            })),
                    )
                    .child(
                        Button::new("btn-add-keypoint")
                            .secondary()
                            .label("+ 重點")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.insert_template_block(
                                    "\\begin{keypoint}\n重點摘要內容。\n\\end{keypoint}",
                                    window,
                                    cx,
                                );
                            })),
                    )
                    .child(
                        Button::new("btn-add-caution")
                            .secondary()
                            .label("+ 安全注意")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.insert_template_block(
                                    "\\begin{caution}\n安全注意事項。\n\\end{caution}",
                                    window,
                                    cx,
                                );
                            })),
                    )
                    .child(
                        Button::new("btn-ins-term")
                            .secondary()
                            .label("\\term")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.append_inline_snippet(
                                    "\\term{比衝}{specific impulse}",
                                    window,
                                    cx,
                                );
                            })),
                    )
                    .child(
                        Button::new("btn-ins-si")
                            .secondary()
                            .label("\\SI")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.append_inline_snippet("\\SI{1.0}{MPa}", window, cx);
                            })),
                    )
                    .child(
                        Button::new("btn-ins-ce")
                            .secondary()
                            .label("\\ce")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.append_inline_snippet("\\ce{H2O2}", window, cx);
                            })),
                    )
                    .child(
                        Button::new("btn-compile")
                            .secondary()
                            .label("XeLaTeX 快速編譯")
                            .on_click(cx.listener(|this, _, _, cx| this.trigger_compile_quick(cx))),
                    )
                    .child(
                        Button::new("btn-toggle-preview")
                            .secondary()
                            .label(if self.show_right_panel && self.right_tab == RightTab::Preview {
                                "隱藏預覽"
                            } else {
                                "PDF 預覽"
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this.show_right_panel && this.right_tab == RightTab::Preview {
                                    this.show_right_panel = false;
                                } else {
                                    this.show_right_panel = true;
                                    this.right_tab = RightTab::Preview;
                                    if this.pdf_image.is_none() {
                                        this.refresh_pdf_preview(true, cx);
                                    }
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .px(px(10.0))
                    .py(px(4.0))
                    .rounded(px(6.0))
                    .bg(BookPalette::accent_soft())
                    .border_1()
                    .border_color(BookPalette::accent())
                    .text_size(px(12.0))
                    .text_color(BookPalette::accent())
                    .child(format!("MCP Live: {}", self.mcp_endpoint)),
            )
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(240.0))
            .h_full()
            .min_h_0()
            .bg(BookPalette::sidebar_bg())
            .border_r_1()
            .border_color(BookPalette::border())
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(12.0))
                    .py(px(10.0))
                    .border_b_1()
                    .border_color(BookPalette::border())
                    .text_size(px(13.0))
                    .text_color(BookPalette::accent())
                    .child("書稿章節目錄 (main.tex)"),
            )
            .child(
                div()
                    .id("chapter-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .p(px(8.0))
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .children(self.chapters.iter().enumerate().map(|(i, ch)| {
                        let is_active =
                            self.active_chapter.as_deref() == Some(ch.relative_path.as_str());
                        let rel = ch.relative_path.clone();
                        div()
                            .id(SharedString::from(format!("ch-{i}")))
                            .px(px(10.0))
                            .py(px(6.0))
                            .rounded(px(6.0))
                            .cursor_pointer()
                            .when(is_active, |el| {
                                el.bg(BookPalette::accent_soft())
                                    .border_1()
                                    .border_color(BookPalette::accent())
                            })
                            .when(!is_active, |el| el.hover(|h| h.bg(BookPalette::border())))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_chapter(&rel, window, cx);
                            }))
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(BookPalette::text_muted())
                                    .child(format!("{} · {}", ch.section_group, ch.relative_path)),
                            )
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .text_color(BookPalette::text_main())
                                    .child(ch.title.clone()),
                            )
                    })),
            )
    }

    fn render_right_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let panel_w = match self.right_tab {
            RightTab::Preview => px(500.0),
            RightTab::Terms | RightTab::Meta | RightTab::Mcp => px(340.0),
        };
        div()
            .w(panel_w)
            .h_full()
            .min_h_0()
            .bg(BookPalette::sidebar_bg())
            .border_l_1()
            .border_color(BookPalette::border())
            .flex()
            .flex_col()
            .child(
                div()
                    .p(px(8.0))
                    .border_b_1()
                    .border_color(BookPalette::border())
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .gap(px(6.0))
                    .child(
                        Button::new("tab-preview")
                            .label("PDF 預覽")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.right_tab = RightTab::Preview;
                                if this.pdf_image.is_none() {
                                    this.refresh_pdf_preview(true, cx);
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("tab-terms")
                            .label(format!("術語/TODO ({})", self.term_issues.len()))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.right_tab = RightTab::Terms;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("tab-meta")
                            .label("封面/作者")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.right_tab = RightTab::Meta;
                                this.sync_meta_inputs(window, cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("tab-mcp")
                            .label(format!("MCP ({})", self.mcp_logs.len()))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.right_tab = RightTab::Mcp;
                                cx.notify();
                            })),
                    ),
            )
            .when(self.right_tab == RightTab::Preview, |el| {
                let page = self.pdf_page;
                let total = self.pdf_total_pages.max(1);
                el.child(
                    div()
                        .px(px(8.0))
                        .py(px(6.0))
                        .border_b_1()
                        .border_color(BookPalette::border())
                        .bg(BookPalette::ribbon_bg())
                        .flex()
                        .flex_row()
                        .items_center()
                        .flex_wrap()
                        .gap(px(4.0))
                        .child(
                            Button::new("pdf-prev10")
                                .secondary()
                                .label("-10")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.set_pdf_page(page.saturating_sub(10).max(1), Some(window), cx);
                                })),
                        )
                        .child(
                            Button::new("pdf-prev")
                                .secondary()
                                .label("上頁")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.set_pdf_page(page.saturating_sub(1).max(1), Some(window), cx);
                                })),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(4.0))
                                .child(
                                    div()
                                        .text_size(px(12.0))
                                        .text_color(BookPalette::text_main())
                                        .child("第"),
                                )
                                .child(Input::new(&self.pdf_page_input).small().w(px(56.0)))
                                .child(
                                    div()
                                        .text_size(px(12.0))
                                        .text_color(BookPalette::text_main())
                                        .child(format!("/ {total} 頁")),
                                )
                                .child(
                                    Button::new("pdf-go")
                                        .secondary()
                                        .label("跳轉")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.jump_pdf_from_input(window, cx);
                                        })),
                                ),
                        )
                        .child(
                            Button::new("pdf-next")
                                .secondary()
                                .label("下頁")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.set_pdf_page((page + 1).min(total), Some(window), cx);
                                })),
                        )
                        .child(
                            Button::new("pdf-next10")
                                .secondary()
                                .label("+10")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.set_pdf_page((page + 10).min(total), Some(window), cx);
                                })),
                        )
                        .child(
                            Button::new("pdf-zoom-out")
                                .secondary()
                                .label("－")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.pdf_dpi = this.pdf_dpi.saturating_sub(25).max(80);
                                    this.refresh_pdf_preview(false, cx);
                                })),
                        )
                        .child(
                            Button::new("pdf-zoom-in")
                                .secondary()
                                .label("＋")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.pdf_dpi = (this.pdf_dpi + 25).min(220);
                                    this.refresh_pdf_preview(false, cx);
                                })),
                        )
                        .child(
                            Button::new("pdf-refresh")
                                .secondary()
                                .label("重整")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.refresh_pdf_preview(true, cx);
                                })),
                        ),
                )
            })
            .child(
                div()
                    .id("right-panel-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .p(px(10.0))
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .map(|el| match self.right_tab {
                        RightTab::Preview => {
                            let mut col = el;
                            if self.pdf_loading {
                                col = col.child(
                                    div()
                                        .text_size(px(12.0))
                                        .text_color(BookPalette::accent())
                                        .child(format!("正在算圖第 {} 頁...", self.pdf_page)),
                                );
                            }
                            if let Some(err) = &self.pdf_error {
                                col = col.child(
                                    div()
                                        .p(px(10.0))
                                        .rounded(px(6.0))
                                        .bg(BookPalette::warn_soft())
                                        .text_size(px(12.0))
                                        .text_color(BookPalette::warn())
                                        .child(err.clone()),
                                );
                            }
                            if let Some(image) = &self.pdf_image {
                                let preview_h = px(650.0 * (self.pdf_dpi as f32 / 130.0));
                                col = col.child(
                                    div()
                                        .w_full()
                                        .bg(BookPalette::paper_bg())
                                        .border_1()
                                        .border_color(BookPalette::border())
                                        .rounded(px(4.0))
                                        .p(px(4.0))
                                        .child(
                                            img(image.clone())
                                                .w_full()
                                                .h(preview_h)
                                                .object_fit(gpui::ObjectFit::Contain),
                                        ),
                                );
                            }
                            col
                        }
                        RightTab::Terms => {
                            if self.term_issues.is_empty() {
                                el.child(
                                    div()
                                        .p(px(10.0))
                                        .rounded(px(6.0))
                                        .bg(BookPalette::case_soft())
                                        .text_size(px(12.5))
                                        .text_color(BookPalette::case_green())
                                        .child("目前章節未發現 terms.csv 避免用詞、\\todo 或 \\needcite。"),
                                )
                            } else {
                                el.children(self.term_issues.iter().enumerate().map(|(i, issue)| {
                                    let issue_clone = issue.clone();
                                    div()
                                        .id(SharedString::from(format!("issue-{i}")))
                                        .p(px(8.0))
                                        .rounded(px(6.0))
                                        .bg(BookPalette::paper_bg())
                                        .border_1()
                                        .border_color(BookPalette::border())
                                        .cursor_pointer()
                                        .hover(|h| h.bg(BookPalette::accent_soft()))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.jump_to_issue(&issue_clone, cx);
                                        }))
                                        .flex()
                                        .flex_col()
                                        .gap(px(3.0))
                                        .child(
                                            div()
                                                .text_size(px(11.5))
                                                .text_color(BookPalette::warn())
                                                .child(format!(
                                                    "[{}] 第 {} 行：{}（點擊跳轉）",
                                                    issue.kind, issue.line, issue.matched
                                                )),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(12.0))
                                                .text_color(BookPalette::text_main())
                                                .child(issue.suggestion.clone()),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(11.0))
                                                .text_color(BookPalette::text_muted())
                                                .child(issue.context.clone()),
                                        )
                                }))
                            }
                        }
                        RightTab::Meta => el
                            .child(
                                div()
                                    .p(px(10.0))
                                    .rounded(px(6.0))
                                    .bg(BookPalette::paper_bg())
                                    .border_1()
                                    .border_color(BookPalette::border())
                                    .flex()
                                    .flex_col()
                                    .gap(px(8.0))
                                    .child(
                                        div()
                                            .text_size(px(13.0))
                                            .text_color(BookPalette::accent())
                                            .child("封面、書名頁與序言署名設定"),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(BookPalette::text_muted())
                                            .child("作者署名（同步至封面、書名頁、PDF 屬性與序言）"),
                                    )
                                    .child(Input::new(&self.meta_author_input).small())
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(BookPalette::text_muted())
                                            .child("出版社名稱（顯示於封面底欄）"),
                                    )
                                    .child(Input::new(&self.meta_publisher_input).small())
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(BookPalette::text_muted())
                                            .child("序言年月（取代 \\todo{年月}）"),
                                    )
                                    .child(Input::new(&self.meta_date_input).small())
                                    .child(
                                        div()
                                            .pt(px(4.0))
                                            .flex()
                                            .flex_row()
                                            .gap(px(8.0))
                                            .child(
                                                Button::new("btn-apply-meta")
                                                    .primary()
                                                    .label("套用並寫回書稿")
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.apply_metadata_to_project(cx);
                                                    })),
                                            ),
                                    ),
                            ),
                        RightTab::Mcp => el
                            .child(
                                div()
                                    .p(px(8.0))
                                    .rounded(px(6.0))
                                    .bg(BookPalette::accent_soft())
                                    .text_size(px(12.0))
                                    .text_color(BookPalette::accent())
                                    .child(format!(
                                        "Streamable HTTP MCP 端點：\n{}\n外部 AI 修改後畫面會自動即時更新。",
                                        self.mcp_endpoint
                                    )),
                            )
                            .children(self.mcp_logs.iter().rev().map(|line| {
                                div()
                                    .p(px(6.0))
                                    .rounded(px(4.0))
                                    .bg(BookPalette::paper_bg())
                                    .border_1()
                                    .border_color(BookPalette::border())
                                    .text_size(px(11.5))
                                    .text_color(BookPalette::text_main())
                                    .child(line.clone())
                            })),
                    }),
            )
    }

    fn render_canvas(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .flex_1()
            .h_full()
            .min_w_0()
            .min_h_0()
            .bg(BookPalette::desk_bg())
            .child(
                list(
                    self.doc_list_state.clone(),
                    cx.processor(|this, idx, _window, cx| {
                        this.render_list_row(idx, cx)
                    }),
                )
                .size_full()
                .pr(px(14.0)),
            )
            .child(Scrollbar::vertical(&self.doc_list_state))
    }

    fn render_list_row(&self, idx: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(block) = self.document.blocks.get(idx) else {
            return div().into_any_element();
        };
        let total = self.document.blocks.len();
        let is_first = idx == 0;
        let is_last = idx + 1 == total;

        div()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .when(is_first, |el| el.pt(px(20.0)))
            .when(is_last, |el| el.pb(px(32.0)))
            .child(
                div()
                    .w(px(820.0))
                    .max_w_full()
                    .bg(BookPalette::paper_bg())
                    .border_l_1()
                    .border_r_1()
                    .when(is_first, |el| el.border_t_1().pt(px(36.0)).rounded_t(px(4.0)))
                    .when(is_last, |el| el.border_b_1().pb(px(48.0)).rounded_b(px(4.0)))
                    .border_color(BookPalette::border())
                    .px(px(52.0))
                    .py(px(6.0))
                    .child(self.render_block_slot(idx, block, cx)),
            )
            .into_any_element()
    }

    fn render_block_slot(
        &self,
        idx: usize,
        block: &Block,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.editing_block == Some(idx) {
            return div()
                .id(SharedString::from(format!("edit-slot-{idx}")))
                .p(px(12.0))
                .rounded(px(6.0))
                .bg(BookPalette::accent_soft())
                .border_2()
                .border_color(BookPalette::accent())
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(BookPalette::accent())
                        .child(format!(
                            "正在編輯區塊 #{}（支援中文輸入法與即時 LaTeX 語法）",
                            idx + 1
                        )),
                )
                .child(Textarea::new(&self.edit_textarea).h(px(140.0)))
                .child(
                    div()
                        .w_full()
                        .p(px(10.0))
                        .rounded(px(4.0))
                        .bg(BookPalette::paper_bg())
                        .border_1()
                        .border_color(BookPalette::border())
                        .flex()
                        .flex_col()
                        .gap(px(6.0))
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(BookPalette::text_muted())
                                .child("即時排版預覽（隨輸入同步更新）"),
                        )
                        .children(
                            self.live_preview_blocks
                                .iter()
                                .map(|b| render_block_visual(&b.kind, false, &self.figure_thumbs)),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(8.0))
                        .child(
                            Button::new("btn-apply-edit")
                                .primary()
                                .label("套用並即時渲染")
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.apply_editing_block(cx)),
                                ),
                        )
                        .child(
                            Button::new("btn-cancel-edit")
                                .secondary()
                                .label("取消")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.editing_block = None;
                                    this.doc_list_state.splice(idx..idx + 1, 1);
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("btn-del-edit")
                                .danger()
                                .label("刪除區塊")
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.delete_selected_block(cx)),
                                ),
                        ),
                )
                .into_any_element();
        }

        let is_selected = self.selected_block == Some(idx);
        let is_expanded = self.expanded_raw.contains(&block.id);
        let block_id = block.id.clone();

        div()
            .id(SharedString::from(format!("blk-{idx}")))
            .w_full()
            .rounded(px(6.0))
            .px(px(8.0))
            .py(px(4.0))
            .cursor_pointer()
            .when(is_selected, |el| {
                el.bg(BookPalette::accent_soft())
                    .border_1()
                    .border_color(BookPalette::accent())
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                if matches!(
                    this.document.blocks.get(idx).map(|b| &b.kind),
                    Some(BlockKind::RawLatex { .. } | BlockKind::Figure { .. })
                ) {
                    if this.selected_block == Some(idx) {
                        this.start_editing_block(idx, window, cx);
                    } else {
                        this.selected_block = Some(idx);
                        if !this.expanded_raw.insert(block_id.clone()) {
                            this.expanded_raw.remove(&block_id);
                        }
                        this.doc_list_state.splice(idx..idx + 1, 1);
                        cx.notify();
                    }
                } else {
                    this.start_editing_block(idx, window, cx);
                }
            }))
            .child(render_block_visual(&block.kind, is_expanded, &self.figure_thumbs))
            .into_any_element()
    }
    fn render_search_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w_full()
            .bg(BookPalette::ribbon_bg())
            .border_b_1()
            .border_color(BookPalette::border())
            .px(px(12.0))
            .py(px(6.0))
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.0))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(BookPalette::accent())
                            .child("搜尋："),
                    )
                    .child(Input::new(&self.search_input).small().w(px(260.0)))
                    .child(
                        Button::new("btn-search-cur")
                            .primary()
                            .label("搜本章")
                            .on_click(cx.listener(|this, _, _, cx| this.run_search(false, cx))),
                    )
                    .child(
                        Button::new("btn-search-all")
                            .secondary()
                            .label("搜全書")
                            .on_click(cx.listener(|this, _, _, cx| this.run_search(true, cx))),
                    )
                    .child(
                        Button::new("btn-search-close")
                            .secondary()
                            .label("關閉")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_search = false;
                                this.search_hits.clear();
                                cx.notify();
                            })),
                    ),
            )
            .when(!self.search_hits.is_empty(), |el| {
                el.child(
                    div()
                        .w_full()
                        .max_h(px(130.0))
                        .id("search-results-box")
                        .overflow_y_scrollbar()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .children(self.search_hits.iter().enumerate().map(|(i, hit)| {
                            let hit_clone = hit.clone();
                            div()
                                .id(SharedString::from(format!("shit-{i}")))
                                .px(px(8.0))
                                .py(px(4.0))
                                .rounded(px(4.0))
                                .bg(BookPalette::paper_bg())
                                .border_1()
                                .border_color(BookPalette::border())
                                .cursor_pointer()
                                .hover(|h| h.bg(BookPalette::accent_soft()))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.jump_to_search_hit(&hit_clone, window, cx);
                                }))
                                .text_size(px(12.0))
                                .text_color(BookPalette::text_main())
                                .child(format!(
                                    "[{} #{}] {}",
                                    hit.chapter_rel, hit.line, hit.snippet
                                ))
                        })),
                )
            })
    }
}

impl Render for TexWordEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &SaveFile, _, cx| {
                this.save_active_file(cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleSearch, window, cx| {
                this.toggle_search(window, cx);
            }))
            .size_full()
            .flex()
            .flex_col()
            .bg(BookPalette::desk_bg())
            .child(self.render_ribbon(cx))
            .when(self.show_search, |el| el.child(self.render_search_bar(cx)))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .child(self.render_sidebar(cx))
                    .child(self.render_canvas(cx))
                    .when(self.show_right_panel, |el| el.child(self.render_right_panel(cx))),
            )
            .child(
                div()
                    .w_full()
                    .px(px(12.0))
                    .py(px(5.0))
                    .bg(BookPalette::ribbon_bg())
                    .border_t_1()
                    .border_color(BookPalette::border())
                    .flex()
                    .flex_row()
                    .justify_between()
                    .text_size(px(12.0))
                    .text_color(BookPalette::text_muted())
                    .child(self.status_message.clone())
                    .child(format!(
                        "章節：{} | 共 {} 個區塊",
                        self.active_chapter.as_deref().unwrap_or("未開啟"),
                        self.document.blocks.len()
                    )),
            )
    }
}

fn render_block_visual(
    kind: &BlockKind,
    expanded_raw: bool,
    figure_thumbs: &HashMap<String, Arc<Image>>,
) -> AnyElement {
    match kind {
        BlockKind::Heading {
            level,
            title,
            label,
            ..
        } => {
            let (font_px, color, bottom_border) = match level {
                HeadingLevel::Chapter => (26.0, BookPalette::accent(), true),
                HeadingLevel::Section => (20.0, BookPalette::text_main(), false),
                HeadingLevel::Subsection => (16.5, BookPalette::text_main(), false),
                HeadingLevel::Subsubsection => (14.5, BookPalette::text_muted(), false),
            };
            div()
                .w_full()
                .pt(px(8.0))
                .pb(px(4.0))
                .when(bottom_border, |el| {
                    el.border_b_2().border_color(BookPalette::accent())
                })
                .flex()
                .flex_row()
                .items_baseline()
                .justify_between()
                .gap(px(8.0))
                .child(
                    div()
                        .flex_1()
                        .text_size(px(font_px))
                        .text_color(color)
                        .child(title.clone()),
                )
                .when_some(label.clone(), |el, lbl| {
                    el.child(
                        div()
                            .text_size(px(11.0))
                            .text_color(BookPalette::text_muted())
                            .child(format!("#{lbl}")),
                    )
                })
                .into_any_element()
        }
        BlockKind::Paragraph { inlines } => render_inlines_styled_text(inlines),
        BlockKind::MathBlock { env, tex, label } => div()
            .w_full()
            .my(px(4.0))
            .px(px(16.0))
            .py(px(12.0))
            .rounded(px(6.0))
            .bg(BookPalette::math_bg())
            .border_1()
            .border_color(BookPalette::border())
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .child(
                div()
                    .flex_1()
                    .flex()
                    .justify_center()
                    .child(render_math_tex(tex, 18.0)),
            )
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(BookPalette::text_muted())
                    .child(match label {
                        Some(l) => format!("({})", l),
                        None => env.env_name().unwrap_or("\\[...\\]").to_string(),
                    }),
            )
            .into_any_element(),
        BlockKind::Callout {
            kind,
            title,
            blocks,
        } => {
            let (accent, bg) = match kind {
                CalloutKind::Caution => (BookPalette::warn(), BookPalette::warn_soft()),
                CalloutKind::Casestudy => (BookPalette::case_green(), BookPalette::case_soft()),
                _ => (BookPalette::accent(), BookPalette::accent_soft()),
            };
            let header = match title {
                Some(t) if !t.is_empty() => format!("【{}】{}", kind.display_label(), t),
                _ => format!("【{}】", kind.display_label()),
            };
            div()
                .w_full()
                .p(px(12.0))
                .rounded(px(6.0))
                .bg(bg)
                .border_l_4()
                .border_color(accent)
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(
                    div()
                        .text_size(px(14.0))
                        .text_color(accent)
                        .child(header),
                )
                .children(
                    blocks
                        .iter()
                        .map(|b| render_block_visual(&b.kind, false, figure_thumbs)),
                )
                .into_any_element()
        }
        BlockKind::Figure {
            path,
            caption,
            source,
            label,
            has_tikzpicture,
            raw,
        } => {
            let thumb = path.as_ref().and_then(|p| figure_thumbs.get(p).cloned());
            div()
                .w_full()
                .p(px(12.0))
                .rounded(px(6.0))
                .bg(BookPalette::math_bg())
                .border_1()
                .border_color(BookPalette::border())
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(13.0))
                                .text_color(BookPalette::accent())
                                .child(if *has_tikzpicture {
                                    "圖表區塊（內嵌 TikZ 向量圖 — 點擊展開 / 再點編輯）"
                                        .to_string()
                                } else {
                                    format!(
                                        "圖表區塊：{}",
                                        path.as_deref().unwrap_or("未指定圖檔")
                                    )
                                }),
                        )
                        .when_some(label.clone(), |el, l| {
                            el.child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(BookPalette::text_muted())
                                    .child(l),
                            )
                        }),
                )
                .when_some(thumb, |el, t| {
                    el.child(
                        div()
                            .w_full()
                            .flex()
                            .justify_center()
                            .bg(BookPalette::paper_bg())
                            .rounded(px(4.0))
                            .p(px(6.0))
                            .child(
                                img(t)
                                    .w(px(520.0))
                                    .max_w_full()
                                    .h(px(260.0))
                                    .object_fit(gpui::ObjectFit::Contain),
                            ),
                    )
                })
                .when_some(caption.clone(), |el, cap| {
                    el.child(
                        div()
                            .text_size(px(13.0))
                            .text_color(BookPalette::text_main())
                            .child(match source {
                                Some(src) => format!("圖說：{cap}（來源：{src}）"),
                                None => format!("圖說：{cap}"),
                            }),
                    )
                })
                .when(expanded_raw, |el| {
                    el.child(
                        div()
                            .p(px(8.0))
                            .rounded(px(4.0))
                            .bg(BookPalette::paper_bg())
                            .text_size(px(11.5))
                            .text_color(BookPalette::text_muted())
                            .child(raw.clone()),
                    )
                })
                .into_any_element()
        }
        BlockKind::List { ordered, items } => div()
            .w_full()
            .pl(px(12.0))
            .flex()
            .flex_col()
            .gap(px(6.0))
            .children(items.iter().enumerate().map(|(i, item)| {
                let bullet = if *ordered {
                    format!("{}.", i + 1)
                } else {
                    "•".to_string()
                };
                div()
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_start()
                    .gap(px(8.0))
                    .child(
                        div()
                            .text_size(px(15.0))
                            .line_height(px(24.0))
                            .text_color(BookPalette::accent())
                            .child(bullet),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(render_inlines_styled_text(item)),
                    )
            }))
            .into_any_element(),
        BlockKind::Comment { lines } => div()
            .w_full()
            .px(px(8.0))
            .py(px(3.0))
            .text_size(px(11.5))
            .text_color(BookPalette::text_muted())
            .child(lines.join("\n"))
            .into_any_element(),
        BlockKind::RawLatex { env_name, raw } => {
            let env = env_name.as_deref().unwrap_or("LaTeX");
            let is_layout_env = matches!(env, "flushright" | "center" | "titlepage" | "quote");
            let has_todo = raw.contains("\\todo") || raw.contains("\\needcite");
            if is_layout_env {
                let mut inner_lines: Vec<&str> = raw.lines().skip(1).collect();
                if inner_lines
                    .last()
                    .is_some_and(|l| l.trim().starts_with("\\end{"))
                {
                    inner_lines.pop();
                }
                let inner_doc = parse_latex(&inner_lines.join("\n"));
                return div()
                    .w_full()
                    .p(px(10.0))
                    .rounded(px(6.0))
                    .bg(BookPalette::sidebar_bg())
                    .border_1()
                    .border_color(if has_todo {
                        BookPalette::todo_red()
                    } else {
                        BookPalette::border()
                    })
                    .flex()
                    .flex_col()
                    .when(env == "flushright", |el| el.items_end())
                    .when(matches!(env, "center" | "titlepage"), |el| el.items_center())
                    .gap(px(4.0))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(BookPalette::text_muted())
                            .child(format!("[{env}]")),
                    )
                    .children(
                        inner_doc
                            .blocks
                            .iter()
                            .map(|b| render_block_visual(&b.kind, false, figure_thumbs)),
                    )
                    .when(expanded_raw, |el| {
                        el.child(
                            div()
                                .w_full()
                                .pt(px(4.0))
                                .text_size(px(11.5))
                                .text_color(BookPalette::text_muted())
                                .child(raw.clone()),
                        )
                    })
                    .into_any_element();
            }

            let preview_lines: String = raw.lines().take(3).collect::<Vec<_>>().join("\n");
            div()
                .w_full()
                .p(px(10.0))
                .rounded(px(6.0))
                .bg(BookPalette::sidebar_bg())
                .border_1()
                .border_color(if has_todo {
                    BookPalette::todo_red()
                } else {
                    BookPalette::border()
                })
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(BookPalette::accent())
                        .child(format!(
                            "原樣保留區塊 [{env}]（點擊展開完整碼 / 再點編輯）"
                        )),
                )
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(BookPalette::text_muted())
                        .child(if expanded_raw {
                            raw.clone()
                        } else {
                            preview_lines
                        }),
                )
                .into_any_element()
        }
    }
}

/// Flatten paragraph/list inlines into a single wrapping `StyledText` with rich highlights.
/// Using one `StyledText` per paragraph enables crisp CJK line wrapping and 10x faster layout.
fn render_inlines_styled_text(inlines: &[Inline]) -> AnyElement {
    let mut text = String::new();
    let mut highlights: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
    collect_inline_runs(inlines, &mut text, &mut highlights, HighlightStyle::default());

    if text.is_empty() {
        text.push(' ');
    }

    div()
        .w_full()
        .text_size(px(15.0))
        .line_height(px(25.0))
        .text_color(BookPalette::text_main())
        .child(StyledText::new(text).with_highlights(highlights))
        .into_any_element()
}

fn collect_inline_runs(
    inlines: &[Inline],
    out: &mut String,
    highlights: &mut Vec<(Range<usize>, HighlightStyle)>,
    inherited: HighlightStyle,
) {
    for inline in inlines {
        match inline {
            Inline::Text { text } => {
                let start = out.len();
                let cleaned = text.replace('~', " ");
                out.push_str(&cleaned);
                if has_style(&inherited) && out.len() > start {
                    highlights.push((start..out.len(), inherited));
                }
            }
            Inline::Math { tex } => {
                let rendered = math_to_plain_text(&parse_math(tex));
                let start = out.len();
                out.push_str(&rendered);
                highlights.push((
                    start..out.len(),
                    HighlightStyle {
                        color: Some(BookPalette::accent()),
                        background_color: Some(BookPalette::math_bg()),
                        font_style: Some(FontStyle::Italic),
                        ..inherited
                    },
                ));
            }
            Inline::Term { zh, en } => {
                let start = out.len();
                out.push_str(&format!("{zh}（{en}）"));
                highlights.push((
                    start..out.len(),
                    HighlightStyle {
                        color: Some(BookPalette::accent()),
                        background_color: Some(BookPalette::accent_soft()),
                        font_weight: Some(FontWeight::SEMIBOLD),
                        ..inherited
                    },
                ));
            }
            Inline::SiUnit { value, unit } => {
                let start = out.len();
                out.push_str(&format!("{value} {unit}"));
                highlights.push((
                    start..out.len(),
                    HighlightStyle {
                        color: Some(BookPalette::text_main()),
                        background_color: Some(BookPalette::sidebar_bg()),
                        ..inherited
                    },
                ));
            }
            Inline::Chem { formula } => {
                let start = out.len();
                out.push_str(&format_chem_unicode(formula));
                highlights.push((
                    start..out.len(),
                    HighlightStyle {
                        color: Some(BookPalette::case_green()),
                        background_color: Some(BookPalette::case_soft()),
                        ..inherited
                    },
                ));
            }
            Inline::Bold { children } => {
                let next = HighlightStyle {
                    font_weight: Some(FontWeight::BOLD),
                    ..inherited
                };
                collect_inline_runs(children, out, highlights, next);
            }
            Inline::Italic { children } => {
                let next = HighlightStyle {
                    font_style: Some(FontStyle::Italic),
                    color: Some(BookPalette::text_muted()),
                    ..inherited
                };
                collect_inline_runs(children, out, highlights, next);
            }
            Inline::Ref { ref_cmd, target } => {
                let start = out.len();
                let label = match ref_cmd.as_str() {
                    "figref" => format!("圖[{target}]"),
                    "tabref" => format!("表[{target}]"),
                    "eqnref" => format!("式[{target}]"),
                    "chapref" => format!("章[{target}]"),
                    "secref" => format!("節[{target}]"),
                    _ => format!("[{target}]"),
                };
                out.push_str(&label);
                highlights.push((
                    start..out.len(),
                    HighlightStyle {
                        color: Some(BookPalette::accent()),
                        background_color: Some(BookPalette::accent_soft()),
                        ..inherited
                    },
                ));
            }
            Inline::Cite { keys } => {
                let start = out.len();
                out.push_str(&format!("[{keys}]"));
                highlights.push((
                    start..out.len(),
                    HighlightStyle {
                        color: Some(BookPalette::text_muted()),
                        background_color: Some(BookPalette::sidebar_bg()),
                        ..inherited
                    },
                ));
            }
            Inline::Todo { text } => {
                let start = out.len();
                out.push_str(&format!("［待補：{text}］"));
                highlights.push((
                    start..out.len(),
                    HighlightStyle {
                        color: Some(BookPalette::todo_red()),
                        background_color: Some(BookPalette::warn_soft()),
                        font_weight: Some(FontWeight::BOLD),
                        ..inherited
                    },
                ));
            }
            Inline::NeedCite => {
                let start = out.len();
                out.push_str("［待查出處］");
                highlights.push((
                    start..out.len(),
                    HighlightStyle {
                        color: Some(BookPalette::todo_red()),
                        background_color: Some(BookPalette::warn_soft()),
                        font_weight: Some(FontWeight::BOLD),
                        ..inherited
                    },
                ));
            }
            Inline::Russian { text } => {
                let start = out.len();
                out.push_str(text);
                highlights.push((
                    start..out.len(),
                    HighlightStyle {
                        color: Some(BookPalette::text_muted()),
                        ..inherited
                    },
                ));
            }
            Inline::Macro { raw } => {
                let start = out.len();
                out.push_str(raw);
                highlights.push((
                    start..out.len(),
                    HighlightStyle {
                        color: Some(BookPalette::text_muted()),
                        ..inherited
                    },
                ));
            }
        }
    }
}

fn has_style(s: &HighlightStyle) -> bool {
    s.color.is_some()
        || s.font_weight.is_some()
        || s.font_style.is_some()
        || s.background_color.is_some()
}
