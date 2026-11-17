use latex_ast::{BookProject, Document, parse_latex, serialize_latex};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EditorSyncEvent {
    ChapterOpened { path: String },
    DocumentUpdated { source: String, summary: String },
    LogMessage { level: String, message: String },
}

#[derive(Debug)]
pub struct EditorSharedState {
    pub project: Option<BookProject>,
    pub active_chapter: Option<String>,
    pub active_file_path: Option<PathBuf>,
    pub document: Document,
    pub dirty: bool,
    pub mcp_logs: Vec<String>,
    pub events_tx: broadcast::Sender<EditorSyncEvent>,
}

impl EditorSharedState {
    pub fn new_shared(initial_project: Option<PathBuf>) -> Arc<RwLock<Self>> {
        let (events_tx, _) = broadcast::channel(128);
        let mut state = Self {
            project: None,
            active_chapter: None,
            active_file_path: None,
            document: Document::new(Vec::new()),
            dirty: false,
            mcp_logs: Vec::new(),
            events_tx,
        };
        if let Some(root) = initial_project {
            let _ = state.open_project(&root);
        }
        Arc::new(RwLock::new(state))
    }

    pub fn open_project(&mut self, root: &PathBuf) -> anyhow::Result<()> {
        let project = BookProject::open(root)?;
        let first = project
            .chapters
            .iter()
            .find(|c| c.section_group == "正文")
            .or_else(|| project.chapters.first())
            .cloned();
        self.project = Some(project);
        if let Some(ch) = first {
            self.open_chapter_path(&ch.relative_path)?;
        }
        Ok(())
    }

    pub fn open_chapter_path(&mut self, relative_or_name: &str) -> anyhow::Result<String> {
        let Some(project) = &self.project else {
            anyhow::bail!("尚未開啟任何書稿專案");
        };
        let (entry, doc) = project.load_chapter(relative_or_name)?;
        self.active_chapter = Some(entry.relative_path.clone());
        self.active_file_path = Some(entry.full_path.clone());
        self.document = doc;
        self.dirty = false;
        let _ = self.events_tx.send(EditorSyncEvent::ChapterOpened {
            path: entry.relative_path.clone(),
        });
        Ok(entry.relative_path)
    }

    pub fn reload_from_disk(&mut self) -> anyhow::Result<()> {
        if let Some(path) = &self.active_file_path {
            let raw = fs::read_to_string(path)?;
            let mut doc = parse_latex(&raw);
            doc.file_path = self.active_chapter.clone();
            self.document = doc;
            self.dirty = false;
        }
        Ok(())
    }

    pub fn save_active_to_disk(&mut self) -> anyhow::Result<PathBuf> {
        let Some(path) = self.active_file_path.clone() else {
            anyhow::bail!("目前沒有開啟中的 .tex 檔案");
        };
        let tex = serialize_latex(&self.document);
        fs::write(&path, tex)?;
        self.dirty = false;
        Ok(path)
    }

    pub fn push_log(&mut self, msg: impl Into<String>) {
        let line = msg.into();
        self.mcp_logs.push(line.clone());
        if self.mcp_logs.len() > 200 {
            let excess = self.mcp_logs.len() - 200;
            self.mcp_logs.drain(0..excess);
        }
        let _ = self.events_tx.send(EditorSyncEvent::LogMessage {
            level: "info".to_string(),
            message: line,
        });
    }
}
