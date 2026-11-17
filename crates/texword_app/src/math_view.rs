use crate::theme::BookPalette;
use gpui::{AnyElement, IntoElement, ParentElement, Styled, div, px};
use math_preview::{MathNode, parse_math};

/// Render a LaTeX math string into native GPUI elements (stacked fractions, scripts, roots).
pub fn render_math_tex(tex: &str, base_px: f32) -> AnyElement {
    let ast = parse_math(tex);
    render_math_node(&ast, base_px)
}

pub fn render_math_node(node: &MathNode, size_px: f32) -> AnyElement {
    let small_px = (size_px * 0.72).max(10.0);
    match node {
        MathNode::Row(items) => div()
            .flex()
            .flex_row()
            .items_center()
            .flex_wrap()
            .gap(px(1.5))
            .children(items.iter().map(|n| render_math_node(n, size_px)))
            .into_any_element(),
        MathNode::Text(s) => div()
            .text_size(px(size_px))
            .text_color(BookPalette::text_main())
            .child(s.clone())
            .into_any_element(),
        MathNode::Fraction { num, den } => div()
            .flex()
            .flex_col()
            .items_center()
            .px(px(3.0))
            .child(
                div()
                    .pb(px(1.5))
                    .border_b_1()
                    .border_color(BookPalette::text_main())
                    .child(render_math_node(num, small_px)),
            )
            .child(div().pt(px(1.5)).child(render_math_node(den, small_px)))
            .into_any_element(),
        MathNode::Sqrt(inner) => div()
            .flex()
            .flex_row()
            .items_center()
            .child(
                div()
                    .text_size(px(size_px))
                    .text_color(BookPalette::text_main())
                    .child("√"),
            )
            .child(
                div()
                    .border_t_1()
                    .border_color(BookPalette::text_main())
                    .px(px(2.0))
                    .child(render_math_node(inner, size_px)),
            )
            .into_any_element(),
        MathNode::Script { base, sub, sup } => {
            let mut row = div()
                .flex()
                .flex_row()
                .items_center()
                .child(render_math_node(base, size_px));

            if sub.is_some() || sup.is_some() {
                let mut scripts = div().flex().flex_col().justify_between().pl(px(1.0));
                if let Some(sp) = sup {
                    scripts = scripts.child(
                        div()
                            .pb(px(2.0))
                            .child(render_math_node(sp, (size_px * 0.65).max(9.5))),
                    );
                }
                if let Some(sb) = sub {
                    scripts = scripts.child(
                        div()
                            .pt(px(2.0))
                            .child(render_math_node(sb, (size_px * 0.65).max(9.5))),
                    );
                }
                row = row.child(scripts);
            }
            row.into_any_element()
        }
        MathNode::Accent { mark, base } => div()
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .text_size(px(small_px))
                    .line_height(px(small_px * 0.7))
                    .text_color(BookPalette::text_main())
                    .child(mark.clone()),
            )
            .child(render_math_node(base, size_px))
            .into_any_element(),
    }
}
