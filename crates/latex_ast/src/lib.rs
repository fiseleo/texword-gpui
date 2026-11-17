pub mod ast;
pub mod book;
pub mod parser;
pub mod serializer;

pub use ast::*;
pub use book::*;
pub use parser::{parse_inlines, parse_latex};
pub use serializer::{serialize_block, serialize_block_kind, serialize_inlines, serialize_latex};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_heading_paragraph_and_equation() {
        let sample = r#"\chapter{推進基礎}\label{ch:01}

液體火箭發動機做的事情可以用一句話說完：\term{比衝}{specific impulse} 是關鍵指標，質量流率為 \SI{2.0}{kg/s}，推進劑為 \ce{H2O2}。

\begin{equation}
  F = \dot{m}\,v_e + (p_e - p_a)\,A_e
  \label{eq:thrust}
\end{equation}

\begin{example}[推力的兩個分量]
動量推力為 \SI{4800}{N}。
\end{example}
"#;

        let doc = parse_latex(sample);
        assert_eq!(doc.blocks.len(), 4);
        assert!(matches!(
            &doc.blocks[0].kind,
            BlockKind::Heading {
                level: HeadingLevel::Chapter,
                title,
                label: Some(lbl),
                ..
            } if title == "推進基礎" && lbl == "ch:01"
        ));
        assert!(matches!(
            &doc.blocks[2].kind,
            BlockKind::MathBlock {
                env: MathEnv::Equation,
                label: Some(lbl),
                ..
            } if lbl == "eq:thrust"
        ));
        assert!(matches!(
            &doc.blocks[3].kind,
            BlockKind::Callout {
                kind: CalloutKind::Example,
                title: Some(t),
                ..
            } if t == "推力的兩個分量"
        ));

        let serialized = serialize_latex(&doc);
        let reparsed = parse_latex(&serialized);
        assert_eq!(doc.blocks.len(), reparsed.blocks.len());
    }
}
