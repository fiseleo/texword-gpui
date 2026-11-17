use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Document {
    pub file_path: Option<String>,
    pub blocks: Vec<Block>,
}

impl Document {
    pub fn new(blocks: Vec<Block>) -> Self {
        Self {
            file_path: None,
            blocks,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Block {
    pub id: String,
    pub kind: BlockKind,
}

impl Block {
    pub fn new(kind: BlockKind) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            kind,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BlockKind {
    Heading {
        level: HeadingLevel,
        starred: bool,
        title: String,
        label: Option<String>,
    },
    Paragraph {
        inlines: Vec<Inline>,
    },
    MathBlock {
        env: MathEnv,
        tex: String,
        label: Option<String>,
    },
    Figure {
        path: Option<String>,
        caption: Option<String>,
        source: Option<String>,
        label: Option<String>,
        has_tikzpicture: bool,
        raw: String,
    },
    Callout {
        kind: CalloutKind,
        title: Option<String>,
        blocks: Vec<Block>,
    },
    List {
        ordered: bool,
        items: Vec<Vec<Inline>>,
    },
    Comment {
        lines: Vec<String>,
    },
    RawLatex {
        env_name: Option<String>,
        raw: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HeadingLevel {
    Chapter,
    Section,
    Subsection,
    Subsubsection,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MathEnv {
    Equation,
    EquationStar,
    Align,
    AlignStar,
    DisplayBracket,
}

impl MathEnv {
    pub fn env_name(&self) -> Option<&'static str> {
        match self {
            Self::Equation => Some("equation"),
            Self::EquationStar => Some("equation*"),
            Self::Align => Some("align"),
            Self::AlignStar => Some("align*"),
            Self::DisplayBracket => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CalloutKind {
    Example,
    Keypoint,
    Caution,
    Casestudy,
    Derivation,
}

impl CalloutKind {
    pub fn env_name(&self) -> &'static str {
        match self {
            Self::Example => "example",
            Self::Keypoint => "keypoint",
            Self::Caution => "caution",
            Self::Casestudy => "casestudy",
            Self::Derivation => "derivation",
        }
    }

    pub fn display_label(&self) -> &'static str {
        match self {
            Self::Example => "例題",
            Self::Keypoint => "重點整理",
            Self::Caution => "安全注意",
            Self::Casestudy => "案例研究",
            Self::Derivation => "公式推導",
        }
    }

    pub fn from_env(name: &str) -> Option<Self> {
        match name {
            "example" => Some(Self::Example),
            "keypoint" => Some(Self::Keypoint),
            "caution" => Some(Self::Caution),
            "casestudy" => Some(Self::Casestudy),
            "derivation" => Some(Self::Derivation),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Inline {
    Text { text: String },
    Math { tex: String },
    Term { zh: String, en: String },
    SiUnit { value: String, unit: String },
    Chem { formula: String },
    Bold { children: Vec<Inline> },
    Italic { children: Vec<Inline> },
    Ref { ref_cmd: String, target: String },
    Cite { keys: String },
    Todo { text: String },
    NeedCite,
    Russian { text: String },
    Macro { raw: String },
}

/// Flatten inline nodes into human-readable preview text (used for outlines/search).
pub fn inlines_plain_text(inlines: &[Inline]) -> String {
    let mut out = String::new();
    for item in inlines {
        match item {
            Inline::Text { text } => out.push_str(text),
            Inline::Math { tex } => {
                out.push('$');
                out.push_str(tex);
                out.push('$');
            }
            Inline::Term { zh, en } => {
                out.push_str(zh);
                out.push('（');
                out.push_str(en);
                out.push('）');
            }
            Inline::SiUnit { value, unit } => {
                out.push_str(value);
                out.push(' ');
                out.push_str(unit);
            }
            Inline::Chem { formula } => out.push_str(formula),
            Inline::Bold { children } | Inline::Italic { children } => {
                out.push_str(&inlines_plain_text(children));
            }
            Inline::Ref { ref_cmd, target } => {
                out.push_str(&format!("[{}:{}]", ref_cmd, target));
            }
            Inline::Cite { keys } => out.push_str(&format!("[{}]", keys)),
            Inline::Todo { text } => out.push_str(&format!("[TODO: {}]", text)),
            Inline::NeedCite => out.push_str("[待補引用]"),
            Inline::Russian { text } => out.push_str(text),
            Inline::Macro { raw } => out.push_str(raw),
        }
    }
    out
}
