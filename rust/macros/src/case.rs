//! serde's `rename_all` rules, applied exactly as `serde_derive` applies
//! them (`serde_derive/src/internals/case.rs`).

use syn::{Error, LitStr};

#[derive(Clone, Copy, Default)]
pub enum RenameRule {
    #[default]
    None,
    Lower,
    Upper,
    Pascal,
    Camel,
    Snake,
    ScreamingSnake,
    Kebab,
    ScreamingKebab,
}

use RenameRule::*;

const RULES: [(&str, RenameRule); 8] = [
    ("lowercase", Lower),
    ("UPPERCASE", Upper),
    ("PascalCase", Pascal),
    ("camelCase", Camel),
    ("snake_case", Snake),
    ("SCREAMING_SNAKE_CASE", ScreamingSnake),
    ("kebab-case", Kebab),
    ("SCREAMING-KEBAB-CASE", ScreamingKebab),
];

impl RenameRule {
    pub fn parse(lit: &LitStr) -> Result<Self, Error> {
        let value = lit.value();
        RULES
            .iter()
            .find(|(name, _)| *name == value)
            .map(|(_, rule)| *rule)
            .ok_or_else(|| {
                let names: Vec<_> = RULES
                    .iter()
                    .map(|(name, _)| format!("\"{name}\""))
                    .collect();
                Error::new(
                    lit.span(),
                    format!("unknown rename rule; expected one of {}", names.join(", ")),
                )
            })
    }

    /// A variant name, written in PascalCase.
    pub fn apply_to_variant(self, variant: &str) -> String {
        match self {
            None | Pascal => variant.to_owned(),
            Lower => variant.to_ascii_lowercase(),
            Upper => variant.to_ascii_uppercase(),
            Camel => variant[..1].to_ascii_lowercase() + &variant[1..],
            Snake => {
                let mut snake = String::new();
                for (i, ch) in variant.char_indices() {
                    if i > 0 && ch.is_uppercase() {
                        snake.push('_');
                    }
                    snake.push(ch.to_ascii_lowercase());
                }
                snake
            }
            ScreamingSnake => Snake.apply_to_variant(variant).to_ascii_uppercase(),
            Kebab => Snake.apply_to_variant(variant).replace('_', "-"),
            ScreamingKebab => ScreamingSnake.apply_to_variant(variant).replace('_', "-"),
        }
    }

    /// A field name, written in snake_case.
    pub fn apply_to_field(self, field: &str) -> String {
        match self {
            None | Lower | Snake => field.to_owned(),
            Upper | ScreamingSnake => field.to_ascii_uppercase(),
            Pascal => {
                let mut pascal = String::new();
                let mut capitalize = true;
                for ch in field.chars() {
                    if ch == '_' {
                        capitalize = true;
                    } else if capitalize {
                        pascal.push(ch.to_ascii_uppercase());
                        capitalize = false;
                    } else {
                        pascal.push(ch);
                    }
                }
                pascal
            }
            Camel => {
                let pascal = Pascal.apply_to_field(field);
                pascal[..1].to_ascii_lowercase() + &pascal[1..]
            }
            Kebab => field.replace('_', "-"),
            ScreamingKebab => field.to_ascii_uppercase().replace('_', "-"),
        }
    }
}
