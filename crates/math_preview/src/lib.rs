use serde::{Deserialize, Serialize};

/// Structured visual math layout AST rendered natively by GPUI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MathNode {
    Row(Vec<MathNode>),
    Text(String),
    Fraction {
        num: Box<MathNode>,
        den: Box<MathNode>,
    },
    Sqrt(Box<MathNode>),
    Script {
        base: Box<MathNode>,
        sub: Option<Box<MathNode>>,
        sup: Option<Box<MathNode>>,
    },
    Accent {
        mark: String,
        base: Box<MathNode>,
    },
}

/// Parse a LaTeX math expression into a renderable `MathNode` tree.
pub fn parse_math(input: &str) -> MathNode {
    let mut parser = MathParser {
        src: input.trim(),
        pos: 0,
    };
    let nodes = parser.parse_sequence(None);
    if nodes.len() == 1 {
        nodes.into_iter().next().unwrap()
    } else {
        MathNode::Row(nodes)
    }
}

/// Format a chemical formula like `H2O2` or `N2O4` with Unicode subscripts for compact inline display.
pub fn format_chem_unicode(formula: &str) -> String {
    let mut out = String::new();
    let mut prev_was_elem = false;
    for ch in formula.chars() {
        if ch.is_ascii_digit() && prev_was_elem {
            out.push(to_subscript_digit(ch));
        } else {
            prev_was_elem = ch.is_ascii_alphabetic() || ch == ')' || ch == ']';
            out.push(ch);
        }
    }
    out
}

fn to_subscript_digit(ch: char) -> char {
    match ch {
        '0' => '₀',
        '1' => '₁',
        '2' => '₂',
        '3' => '₃',
        '4' => '₄',
        '5' => '₅',
        '6' => '₆',
        '7' => '₇',
        '8' => '₈',
        '9' => '₉',
        _ => ch,
    }
}

struct MathParser<'a> {
    src: &'a str,
    pos: usize,
}

impl<'a> MathParser<'a> {
    fn rest(&self) -> &'a str {
        &self.src[self.pos..]
    }

    fn skip_ws(&mut self) {
        while let Some(ch) = self.rest().chars().next() {
            if ch.is_whitespace() {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }
    }

    fn parse_sequence(&mut self, stop_char: Option<char>) -> Vec<MathNode> {
        let mut nodes = Vec::new();
        while !self.rest().is_empty() {
            self.skip_ws();
            if self.rest().is_empty() {
                break;
            }
            if let Some(stop) = stop_char
                && self.rest().starts_with(stop)
            {
                self.pos += stop.len_utf8();
                break;
            }

            let atom = self.parse_atom();
            let atom = self.attach_scripts(atom);
            nodes.push(atom);
        }
        nodes
    }

    fn attach_scripts(&mut self, mut base: MathNode) -> MathNode {
        let mut sub: Option<Box<MathNode>> = None;
        let mut sup: Option<Box<MathNode>> = None;

        loop {
            self.skip_ws();
            if self.rest().starts_with('_') && sub.is_none() {
                self.pos += 1;
                sub = Some(Box::new(self.parse_atom()));
            } else if self.rest().starts_with('^') && sup.is_none() {
                self.pos += 1;
                sup = Some(Box::new(self.parse_atom()));
            } else {
                break;
            }
        }

        if sub.is_some() || sup.is_some() {
            base = MathNode::Script {
                base: Box::new(base),
                sub,
                sup,
            };
        }
        base
    }

    fn parse_atom(&mut self) -> MathNode {
        self.skip_ws();
        let rest = self.rest();
        if rest.is_empty() {
            return MathNode::Text(String::new());
        }

        if rest.starts_with('{') {
            self.pos += 1;
            let inner = self.parse_sequence(Some('}'));
            if inner.len() == 1 {
                return inner.into_iter().next().unwrap();
            }
            return MathNode::Row(inner);
        }

        if rest.starts_with('\\') {
            return self.parse_command();
        }

        let ch = rest.chars().next().unwrap();
        self.pos += ch.len_utf8();
        let s = match ch {
            '-' => "−".to_string(),
            '*' => "·".to_string(),
            _ => ch.to_string(),
        };
        MathNode::Text(s)
    }

    fn parse_command(&mut self) -> MathNode {
        self.pos += 1; // skip '\'
        let rest = self.rest();
        if rest.is_empty() {
            return MathNode::Text("\\".to_string());
        }

        // Non-alpha single char command like \, \; \! \{ \} \\
        let first = rest.chars().next().unwrap();
        if !first.is_ascii_alphabetic() {
            self.pos += first.len_utf8();
            let sym = match first {
                ',' | ';' | ':' => " ",
                '!' => "",
                '{' => "{",
                '}' => "}",
                '\\' => " ↵ ",
                ' ' => " ",
                _ => return MathNode::Text(first.to_string()),
            };
            return MathNode::Text(sym.to_string());
        }

        let len = rest
            .char_indices()
            .find(|(_, c)| !c.is_ascii_alphabetic())
            .map(|(idx, _)| idx)
            .unwrap_or(rest.len());
        let cmd = &rest[..len];
        self.pos += len;

        match cmd {
            "frac" | "dfrac" | "tfrac" => {
                let num = Box::new(self.parse_atom());
                let den = Box::new(self.parse_atom());
                MathNode::Fraction { num, den }
            }
            "sqrt" => {
                self.skip_ws();
                if self.rest().starts_with('[') {
                    // skip optional root degree for simple preview
                    if let Some(end) = self.rest().find(']') {
                        self.pos += end + 1;
                    }
                }
                let inner = Box::new(self.parse_atom());
                MathNode::Sqrt(inner)
            }
            "dot" => MathNode::Accent {
                mark: "˙".to_string(),
                base: Box::new(self.parse_atom()),
            },
            "ddot" => MathNode::Accent {
                mark: "¨".to_string(),
                base: Box::new(self.parse_atom()),
            },
            "bar" | "overline" => MathNode::Accent {
                mark: "¯".to_string(),
                base: Box::new(self.parse_atom()),
            },
            "vec" => MathNode::Accent {
                mark: "→".to_string(),
                base: Box::new(self.parse_atom()),
            },
            "hat" => MathNode::Accent {
                mark: "^".to_string(),
                base: Box::new(self.parse_atom()),
            },
            "text" | "mathrm" | "mathbf" | "mathit" | "bm" | "operatorname" => self.parse_atom(),
            "left" | "right" => {
                self.skip_ws();
                if let Some(ch) = self.rest().chars().next() {
                    if ch == '.' {
                        self.pos += 1;
                        return MathNode::Text(String::new());
                    }
                    if ch == '\\' {
                        return self.parse_command();
                    }
                    self.pos += ch.len_utf8();
                    return MathNode::Text(ch.to_string());
                }
                MathNode::Text(String::new())
            }
            _ => MathNode::Text(lookup_symbol(cmd).to_string()),
        }
    }
}

fn lookup_symbol(cmd: &str) -> &str {
    match cmd {
        // Greek lowercase
        "alpha" => "α",
        "beta" => "β",
        "gamma" => "γ",
        "delta" => "δ",
        "epsilon" | "varepsilon" => "ε",
        "zeta" => "ζ",
        "eta" => "η",
        "theta" | "vartheta" => "θ",
        "iota" => "ι",
        "kappa" => "κ",
        "lambda" => "λ",
        "mu" => "μ",
        "nu" => "ν",
        "xi" => "ξ",
        "pi" => "π",
        "rho" | "varrho" => "ρ",
        "sigma" => "σ",
        "tau" => "τ",
        "upsilon" => "υ",
        "phi" | "varphi" => "φ",
        "chi" => "χ",
        "psi" => "ψ",
        "omega" => "ω",
        // Greek uppercase
        "Gamma" => "Γ",
        "Delta" => "Δ",
        "Theta" => "Θ",
        "Lambda" => "Λ",
        "Xi" => "Ξ",
        "Pi" => "Π",
        "Sigma" => "Σ",
        "Phi" => "Φ",
        "Psi" => "Ψ",
        "Omega" => "Ω",
        // Operators & relations
        "cdot" => "·",
        "times" => "×",
        "div" => "÷",
        "pm" => "±",
        "mp" => "∓",
        "approx" => "≈",
        "equiv" => "≡",
        "neq" | "ne" => "≠",
        "le" | "leq" => "≤",
        "ge" | "geq" => "≥",
        "ll" => "≪",
        "gg" => "≫",
        "propto" => "∝",
        "infty" => "∞",
        "partial" => "∂",
        "nabla" => "∇",
        "sum" => "∑",
        "prod" => "∏",
        "int" => "∫",
        "oint" => "∮",
        "to" | "rightarrow" => "→",
        "leftarrow" => "←",
        "Rightarrow" => "⇒",
        "leftrightarrow" => "↔",
        "quad" => "  ",
        "qquad" => "    ",
        "ln" => "ln",
        "log" => "log",
        "exp" => "exp",
        "sin" => "sin",
        "cos" => "cos",
        "tan" => "tan",
        _ => cmd,
    }
}

/// Convert a `MathNode` into compact plain Unicode text (helpful for inline search/MCP summaries).
pub fn math_to_plain_text(node: &MathNode) -> String {
    match node {
        MathNode::Row(items) => items.iter().map(math_to_plain_text).collect::<Vec<_>>().join(""),
        MathNode::Text(s) => s.clone(),
        MathNode::Fraction { num, den } => {
            format!("({})/({})", math_to_plain_text(num), math_to_plain_text(den))
        }
        MathNode::Sqrt(inner) => format!("√({})", math_to_plain_text(inner)),
        MathNode::Script { base, sub, sup } => {
            let mut s = math_to_plain_text(base);
            if let Some(sb) = sub {
                s.push('_');
                s.push_str(&math_to_plain_text(sb));
            }
            if let Some(sp) = sup {
                s.push('^');
                s.push_str(&math_to_plain_text(sp));
            }
            s
        }
        MathNode::Accent { mark, base } => format!("{}{}", math_to_plain_text(base), mark),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_thrust_equation() {
        let node = parse_math(r"c = \frac{F}{\dot{m}} = v_e + \frac{(p_e - p_a)\,A_e}{\dot{m}}");
        let plain = math_to_plain_text(&node);
        assert!(plain.contains("(F)/(m˙)"));
        assert!(plain.contains("v_e"));
    }

    #[test]
    fn formats_chemical_formula() {
        assert_eq!(format_chem_unicode("H2O2"), "H₂O₂");
    }
}
