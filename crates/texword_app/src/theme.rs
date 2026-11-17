use gpui::{Hsla, rgb};

/// Color palette matching Microsoft Word paper aesthetics + `style/rocketbook.sty`.
pub struct BookPalette;

impl BookPalette {
    pub fn desk_bg() -> Hsla {
        rgb(0xEEF1F5).into()
    }
    pub fn paper_bg() -> Hsla {
        rgb(0xFFFFFF).into()
    }
    pub fn sidebar_bg() -> Hsla {
        rgb(0xF7F8FA).into()
    }
    pub fn ribbon_bg() -> Hsla {
        rgb(0xFBFBFC).into()
    }
    pub fn border() -> Hsla {
        rgb(0xDCE1E8).into()
    }
    pub fn text_main() -> Hsla {
        rgb(0x1E242B).into()
    }
    pub fn text_muted() -> Hsla {
        rgb(0x64707D).into()
    }
    /// rocketbook.sty `rbaccent` (#1F4E79)
    pub fn accent() -> Hsla {
        rgb(0x1F4E79).into()
    }
    pub fn accent_soft() -> Hsla {
        rgb(0xEBF3FA).into()
    }
    /// rocketbook.sty `rbwarn` (#9C2A00)
    pub fn warn() -> Hsla {
        rgb(0x9C2A00).into()
    }
    pub fn warn_soft() -> Hsla {
        rgb(0xFDF1EC).into()
    }
    /// rocketbook.sty `rbcase` (#3C5A14)
    pub fn case_green() -> Hsla {
        rgb(0x3C5A14).into()
    }
    pub fn case_soft() -> Hsla {
        rgb(0xF1F6EA).into()
    }
    /// rocketbook.sty `rbtodo` (#C00000)
    pub fn todo_red() -> Hsla {
        rgb(0xC00000).into()
    }
    pub fn math_bg() -> Hsla {
        rgb(0xF8FAFC).into()
    }
}
