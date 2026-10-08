//! Regex literals.
//!
//! A rulekit regex literal (`/pattern/flags` or `|pattern|flags`) is written
//! in rulekit's regex dialect: the RE2 syntax that both Go's `regexp` and the
//! `regex` crate accept, minus the forms that engines read differently, with
//! the same meaning in every implementation. Compiling a literal:
//!
//! 1. [`literal_pattern`] extracts the pattern and turns the flags into a
//!    `(?flags)` prefix (Go `parseRegex`).
//! 2. [`check_regex_dialect`] rejects ambiguous forms (Go `checkRegexDialect`).
//! 3. `regex_syntax` parses the pattern and [`Dialect`] walks the syntax tree:
//!    it rejects the syntax Go's `regexp` does not accept and rewrites the
//!    classes whose meaning differs between the engines.
//! 4. The `regex` crate compiles the rewritten pattern.

use regex_syntax::ast::{self, Ast};

/// Nesting limit of the syntax tree (the `regex` crate's default). Patterns
/// within [`MAX_GROUP_DEPTH`] stay below it: each group level adds at most four
/// tree levels (group, alternation, concatenation, repetition).
const NEST_LIMIT: u32 = 250;

/// Nesting a rewrite adds below a node (`\B` becomes four levels).
const REWRITE_NEST: u32 = 4;

/// Deepest group nesting: `(` (including `(?flags)`) may be open at most this
/// many times at any point.
const MAX_GROUP_DEPTH: u32 = 50;

/// Largest repetition count, and largest product of the counts of nested
/// counted repetitions (Go's limit).
const MAX_REPEAT: u32 = 1000;

/// Compiled size limit: Go's budget for a compiled regexp (`regexp/syntax`
/// sizes its limits from 128 MB), so that patterns such as `\pL{1000}` compile.
const PROGRAM_BUDGET: usize = 128 << 20;

/// Compiles a regex literal token as lexed: `/pattern/flags` or
/// `|pattern|flags`, where flags are zero or more of `i`, `m`, `s`.
pub(crate) fn compile_literal(raw: &str) -> Result<regex::Regex, String> {
    let pattern = literal_pattern(raw)?;
    check_regex_dialect(&pattern)?;
    let rewritten = rewrite(&pattern)?;
    regex::RegexBuilder::new(&rewritten)
        .nest_limit(NEST_LIMIT + REWRITE_NEST)
        .size_limit(PROGRAM_BUDGET)
        .build()
        .map_err(|err| err.to_string())
}

/// Port of the pattern extraction in Go `parseRegex`: the pattern ends at the
/// last occurrence of the opening delimiter, and flags become a `(?flags)`
/// prefix.
fn literal_pattern(raw: &str) -> Result<String, String> {
    let bytes = raw.as_bytes();
    let Some(&delim) = bytes.first() else {
        return Err("empty regex literal".to_owned());
    };
    let end = bytes.iter().rposition(|&b| b == delim).unwrap_or(0);
    if end == 0 {
        return Err("regex literal is missing its closing delimiter".to_owned());
    }
    let pattern = &raw[1..end];
    let flags = &raw[end + 1..];
    if flags.is_empty() {
        return Ok(pattern.to_owned());
    }
    for flag in ['i', 'm', 's'] {
        if flags.matches(flag).count() > 1 {
            return Err(format!("duplicate regex flag '{flag}'"));
        }
    }
    Ok(format!("(?{flags}){pattern}"))
}

/// Port of Go `checkRegexDialect`: rejects regex forms that regex engines
/// interpret differently, so a pattern means the same thing in every rulekit
/// implementation.
fn check_regex_dialect(pattern: &str) -> Result<(), String> {
    let p = pattern.as_bytes();
    let mut in_class = false;
    let mut class_start = 0;
    let mut i = 0;
    while i < p.len() {
        let c = p[i];
        if c == b'\\' {
            if i + 1 >= p.len() {
                return Ok(()); // the compiler reports the trailing backslash
            }
            let next = p[i + 1];
            let n = next as char;
            match next {
                b'<' | b'>' => {
                    return Err(format!(
                        r"\{n} is not supported; use \b for word boundaries"
                    ));
                }
                b'Q' | b'E' => {
                    return Err(
                        r"\Q...\E is not supported; escape each character instead".to_owned()
                    );
                }
                b'0'..=b'9' => {
                    return Err(format!(
                        r"\{n} is not supported; use \x{{...}} for character codes"
                    ));
                }
                b'b' | b'B' if p.get(i + 2) == Some(&b'{') => {
                    return Err(format!(r"\{n}{{...}} is not supported"));
                }
                b'p' | b'P' if p[i + 2..].starts_with(b"{^") => {
                    return Err(format!(
                        r"\{n}{{^...}} is not supported; use \P{{...}} to negate a Unicode class"
                    ));
                }
                _ => {}
            }
            let mut end = i + 1;
            if matches!(next, b'p' | b'P' | b'x')
                && p.get(i + 2) == Some(&b'{')
                && let Some(close) = p[i + 2..].iter().position(|&b| b == b'}')
            {
                end = i + 2 + close;
            }
            if in_class
                && b"dDsSwWpP".contains(&next)
                && end + 2 < p.len()
                && p[end + 1] == b'-'
                && p[end + 2] != b']'
            {
                return Err(format!(
                    r"\{n} cannot start a range in a character class; escape the dash as \-"
                ));
            }
            i = end + 1;
            continue;
        }

        if in_class {
            if c == b']' && i > class_start {
                in_class = false;
            } else if c == b'[' {
                if p[i..].starts_with(b"[:")
                    && let Some(close) = find(&p[i + 2..], b":]")
                {
                    i += 2 + close + 2;
                    continue;
                }
                return Err(
                    r"nested character classes are not supported; escape [ as \[".to_owned(),
                );
            } else if matches!(c, b'&' | b'-' | b'~') && p.get(i + 1) == Some(&c) {
                let c = c as char;
                return Err(format!(
                    r"{c}{c} in a character class is not supported; escape it as \{c}\{c}"
                ));
            }
            i += 1;
            continue;
        }

        if c == b'[' {
            in_class = true;
            class_start = i + 1;
            if class_start < p.len() && p[class_start] == b'^' {
                class_start += 1;
            }
        } else if c == b'{' && p.get(i + 1) == Some(&b',') {
            return Err("{,n} is not supported; use {0,n}".to_owned());
        } else if c == b'{' && !is_repetition(&p[i..]) {
            return Err(
                r"{ must start a repetition such as {2} or {1,3}; escape a literal brace as \{"
                    .to_owned(),
            );
        }
        i += 1;
    }
    Ok(())
}

/// Port of Go `isRepetition`: whether s starts with `{n}`, `{n,}`, or `{n,m}`.
fn is_repetition(s: &[u8]) -> bool {
    let mut i = skip_digits(s, 1);
    if i == 1 {
        return false;
    }
    if i < s.len() && s[i] == b',' {
        i = skip_digits(s, i + 1);
    }
    i < s.len() && s[i] == b'}'
}

fn skip_digits(s: &[u8], mut i: usize) -> usize {
    while i < s.len() && s[i].is_ascii_digit() {
        i += 1;
    }
    i
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Parses `pattern`, checks it against the dialect, and returns it rewritten
/// for the `regex` crate.
fn rewrite(pattern: &str) -> Result<String, String> {
    let ast = ast::parse::ParserBuilder::new()
        .nest_limit(NEST_LIMIT)
        .build()
        .parse(pattern)
        .map_err(|err| syntax_error(pattern, err.span(), &err.kind().to_string()))?;
    ast::visit(
        &ast,
        Dialect {
            pattern,
            out: String::with_capacity(pattern.len()),
            copied: 0,
            repeat_budgets: Vec::new(),
            group_depth: 0,
        },
    )
}

/// An error in Go's format: `error parsing regexp: message: `text``.
fn syntax_error(pattern: &str, span: &ast::Span, message: &str) -> String {
    let text = &pattern[span.start.offset..span.end.offset];
    format!("error parsing regexp: {message}: `{text}`")
}

/// Syntax-tree walk that enforces the dialect and builds the rewritten
/// pattern.
///
/// Rejected, because Go's `regexp` does not accept them:
/// - flags other than `i`, `m`, `s`, `U`;
/// - capture names with characters other than ASCII letters, digits, `_`;
/// - `\u`/`\U` escapes, `\<`, `\>`, `\b{...}`, nested classes, and class set
///   operations (`&&`, `--`, `~~`);
/// - a repetition operator applied to a repetition (`a**`, `a{2}{3}`);
/// - counts above [`MAX_REPEAT`], and nested counted repetitions whose
///   counts multiply to more than [`MAX_REPEAT`]: walking down from a counted
///   repetition with a count of 2 or more, the budget starts at
///   [`MAX_REPEAT`] and is divided by each count (the maximum, or the minimum
///   when unbounded); a count larger than the remaining budget is an error, and
///   a count of 0 ends the walk for that subtree.
///
/// Rejected, although Go's `regexp` accepts them:
/// - counts with a leading zero (`a{01}`, literal text in Go);
/// - a class starting with `]-` (`[]-a]`, a range from `]` in Go);
/// - Unicode class names missing from [`UNICODE_CLASSES`];
/// - groups nested deeper than [`MAX_GROUP_DEPTH`].
///
/// Rewritten, so both engines agree:
/// - `\d \s \w` and their negations become Go's ASCII sets (`\s` is
///   `[\t\n\f\r ]`). Under `(?i)` both engines case fold the set and then
///   negate it, so `(?i)\w` matches U+212A KELVIN SIGN and U+017F LATIN SMALL
///   LETTER LONG S, and `(?i)\W` matches neither.
/// - `\b` and `\B` use ASCII word characters and only hold between
///   characters.
/// - Unicode class names become explicit `gc=`/`sc=` queries.
struct Dialect<'p> {
    pattern: &'p str,
    out: String,
    /// `pattern[..copied]` has been written to `out`.
    copied: usize,
    /// For each enclosing repetition, the remaining count budget (`None`: no
    /// enclosing counted repetition limits the counts below it).
    repeat_budgets: Vec<Option<u32>>,
    /// Number of enclosing groups.
    group_depth: u32,
}

impl Dialect<'_> {
    fn error(&self, span: &ast::Span, message: &str) -> String {
        syntax_error(self.pattern, span, message)
    }

    /// Writes `parts` in place of the pattern text at `span`. Spans are
    /// replaced in pattern order.
    fn replace(&mut self, span: &ast::Span, parts: &[&str]) {
        self.out
            .push_str(&self.pattern[self.copied..span.start.offset]);
        for part in parts {
            self.out.push_str(part);
        }
        self.copied = span.end.offset;
    }

    fn enter_group(&mut self, group: &ast::Group) -> Result<(), String> {
        let open = ast::Span::new(group.span.start, group.ast.span().start);
        self.group_depth += 1;
        if self.group_depth > MAX_GROUP_DEPTH {
            return Err(self.error(&open, "expression nests too deeply"));
        }
        match &group.kind {
            ast::GroupKind::NonCapturing(flags) => self.check_flags(flags),
            ast::GroupKind::CaptureName { name, .. } => {
                let valid = name
                    .name
                    .bytes()
                    .all(|b| b == b'_' || b.is_ascii_alphanumeric());
                if valid {
                    Ok(())
                } else {
                    Err(self.error(&name.span, "invalid named capture"))
                }
            }
            ast::GroupKind::CaptureIndex(_) => Ok(()),
        }
    }

    fn check_flags(&self, flags: &ast::Flags) -> Result<(), String> {
        for item in &flags.items {
            if let ast::FlagsItemKind::Flag(
                ast::Flag::Unicode | ast::Flag::CRLF | ast::Flag::IgnoreWhitespace,
            ) = item.kind
            {
                return Err(self.error(&item.span, "invalid or unsupported Perl syntax"));
            }
        }
        Ok(())
    }

    fn check_literal(&self, literal: &ast::Literal) -> Result<(), String> {
        use ast::{HexLiteralKind, LiteralKind, SpecialLiteralKind};
        match &literal.kind {
            LiteralKind::Verbatim
            | LiteralKind::Meta
            | LiteralKind::Superfluous
            | LiteralKind::HexFixed(HexLiteralKind::X)
            | LiteralKind::HexBrace(HexLiteralKind::X) => Ok(()),
            LiteralKind::Special(kind) if *kind != SpecialLiteralKind::Space => Ok(()),
            _ => Err(self.error(&literal.span, "invalid escape sequence")),
        }
    }

    fn check_repetition(&mut self, rep: &ast::Repetition) -> Result<(), String> {
        if let Ast::Repetition(inner) = &*rep.ast {
            let span = ast::Span::new(inner.op.span.start, rep.op.span.end);
            return Err(self.error(&span, "invalid nested repetition operator"));
        }
        let budget = self.repeat_budgets.last().copied().flatten();
        let budget = match &rep.op.kind {
            ast::RepetitionKind::Range(range) => {
                let (min, max) = match *range {
                    ast::RepetitionRange::Exactly(n) => (n, Some(n)),
                    ast::RepetitionRange::AtLeast(n) => (n, None),
                    ast::RepetitionRange::Bounded(min, max) => (min, Some(max)),
                };
                // Go reads a count with a leading zero (`a{01}`) as literal text.
                let text = &self.pattern[rep.op.span.start.offset..rep.op.span.end.offset];
                if text
                    .split(|c: char| !c.is_ascii_digit())
                    .any(|n| n.len() > 1 && n.starts_with('0'))
                {
                    return Err(self.error(&rep.op.span, "invalid repeat count"));
                }
                if min > MAX_REPEAT || max.is_some_and(|max| max > MAX_REPEAT) {
                    return Err(self.error(&rep.op.span, "invalid repeat count"));
                }
                match max.unwrap_or(min) {
                    0 => None,
                    count => match budget.or((count >= 2).then_some(MAX_REPEAT)) {
                        Some(budget) if count > budget => {
                            return Err(self.error(&rep.span, "invalid repeat count"));
                        }
                        budget => budget.map(|budget| budget / count),
                    },
                }
            }
            _ => budget,
        };
        self.repeat_budgets.push(budget);
        Ok(())
    }

    /// Rejects `[]-a]`: Go reads a leading `]` followed by `-` as the start of
    /// a range, `regex_syntax` as two literals.
    fn check_class_start(&self, class: &ast::ClassBracketed) -> Result<(), String> {
        let text = &self.pattern[class.span.start.offset + 1..class.span.end.offset];
        let text = text.strip_prefix('^').unwrap_or(text);
        if text.starts_with("]-") && !text[2..].starts_with(']') {
            return Err(self.error(&class.span, "invalid character class range"));
        }
        Ok(())
    }

    fn rewrite_unicode_class(&mut self, class: &ast::ClassUnicode) -> Result<(), String> {
        let target = match &class.kind {
            ast::ClassUnicodeKind::OneLetter(c) => unicode_class_target(c.encode_utf8(&mut [0; 4])),
            ast::ClassUnicodeKind::Named(name) => unicode_class_target(name),
            ast::ClassUnicodeKind::NamedValue { .. } => None,
        };
        let Some(target) = target else {
            return Err(self.error(&class.span, "invalid character class range"));
        };
        let open = if class.negated { r"\P{" } else { r"\p{" };
        self.replace(&class.span, &[open, target, "}"]);
        Ok(())
    }
}

impl ast::Visitor for Dialect<'_> {
    type Output = String;
    type Err = String;

    fn finish(mut self) -> Result<String, String> {
        self.out.push_str(&self.pattern[self.copied..]);
        Ok(self.out)
    }

    fn visit_pre(&mut self, ast: &Ast) -> Result<(), String> {
        match ast {
            Ast::Flags(set) => {
                // `(?flags)` counts as an opening parenthesis.
                if self.group_depth >= MAX_GROUP_DEPTH {
                    return Err(self.error(&set.span, "expression nests too deeply"));
                }
                self.check_flags(&set.flags)
            }
            Ast::Group(group) => self.enter_group(group),
            Ast::Repetition(rep) => self.check_repetition(rep),
            Ast::Literal(literal) => self.check_literal(literal),
            Ast::Assertion(assertion) => {
                match assertion.kind {
                    ast::AssertionKind::StartLine
                    | ast::AssertionKind::EndLine
                    | ast::AssertionKind::StartText
                    | ast::AssertionKind::EndText => {}
                    ast::AssertionKind::WordBoundary => {
                        self.replace(&assertion.span, &[r"(?-u:\b)"]);
                    }
                    // `(?-u:\B)` alone also holds inside a UTF-8 sequence, and
                    // the `regex` crate then misses matches (`\B|\x{212A}\b`
                    // on "a\u{212A}b"); the Unicode `\b|\B` only holds
                    // between characters.
                    ast::AssertionKind::NotWordBoundary => {
                        self.replace(&assertion.span, &[r"(?:(?-u:\B)(?:\b|\B))"]);
                    }
                    _ => return Err(self.error(&assertion.span, "invalid escape sequence")),
                }
                Ok(())
            }
            Ast::ClassPerl(class) => {
                self.replace(&class.span, &[perl_class(class, false)]);
                Ok(())
            }
            Ast::ClassUnicode(class) => self.rewrite_unicode_class(class),
            Ast::ClassBracketed(class) => self.check_class_start(class),
            Ast::Empty(_) | Ast::Dot(_) | Ast::Alternation(_) | Ast::Concat(_) => Ok(()),
        }
    }

    fn visit_post(&mut self, ast: &Ast) -> Result<(), String> {
        match ast {
            Ast::Repetition(_) => {
                self.repeat_budgets.pop();
            }
            Ast::Group(_) => self.group_depth -= 1,
            _ => {}
        }
        Ok(())
    }

    fn visit_class_set_item_pre(&mut self, item: &ast::ClassSetItem) -> Result<(), String> {
        match item {
            ast::ClassSetItem::Literal(literal) => self.check_literal(literal),
            ast::ClassSetItem::Range(range) => {
                self.check_literal(&range.start)?;
                self.check_literal(&range.end)
            }
            ast::ClassSetItem::Perl(class) => {
                self.replace(&class.span, &[perl_class(class, true)]);
                Ok(())
            }
            ast::ClassSetItem::Unicode(class) => self.rewrite_unicode_class(class),
            ast::ClassSetItem::Bracketed(class) => {
                Err(self.error(&class.span, "invalid character class"))
            }
            ast::ClassSetItem::Empty(_)
            | ast::ClassSetItem::Ascii(_)
            | ast::ClassSetItem::Union(_) => Ok(()),
        }
    }

    fn visit_class_set_binary_op_pre(&mut self, op: &ast::ClassSetBinaryOp) -> Result<(), String> {
        Err(self.error(&op.span, "invalid character class"))
    }
}

/// Go's ASCII Perl classes, written for use outside or inside brackets.
/// Inside brackets the negated classes are POSIX classes or explicit ranges,
/// which `regex_syntax` (like Go) case folds before negating.
fn perl_class(class: &ast::ClassPerl, in_brackets: bool) -> &'static str {
    use ast::ClassPerlKind::{Digit, Space, Word};
    match (&class.kind, class.negated, in_brackets) {
        (Digit, false, false) => "[0-9]",
        (Digit, true, false) => "[^0-9]",
        (Digit, false, true) => "[:digit:]",
        (Digit, true, true) => "[:^digit:]",
        (Space, false, false) => r"[\t\n\f\r ]",
        (Space, true, false) => r"[^\t\n\f\r ]",
        (Space, false, true) => r"\t\n\f\r ",
        (Space, true, true) => r"\x00-\x08\x0B\x0E-\x1F!-\x{10FFFF}",
        (Word, false, false) => "[0-9A-Za-z_]",
        (Word, true, false) => "[^0-9A-Za-z_]",
        (Word, false, true) => "[:word:]",
        (Word, true, true) => "[:^word:]",
    }
}

/// The `regex_syntax` query for a Unicode class name, with Go's spelling
/// rules: ASCII letters match regardless of case, and spaces, underscores,
/// and hyphens are ignored.
fn unicode_class_target(name: &str) -> Option<&'static str> {
    let key: String = name
        .chars()
        .filter(|c| !matches!(c, ' ' | '_' | '-'))
        .map(|c| c.to_ascii_lowercase())
        .collect();
    UNICODE_CLASSES
        .binary_search_by_key(&key.as_str(), |&(name, _)| name)
        .ok()
        .map(|i| UNICODE_CLASSES[i].1)
}

/// Unicode class names accepted by both Go 1.27.1 `regexp` and
/// `regex_syntax` 0.8 with the same meaning, keyed by the name in lowercase
/// without spaces, underscores, and hyphens, and sorted by key. The values are
/// `regex_syntax` queries.
///
/// These are Go's general categories, their long aliases, and Go's scripts,
/// plus `Any`, `ASCII`, and `Assigned`. Left out: `Cs`/`Surrogate`
/// (`regex_syntax` has no surrogate class), `LC`/`Cased_Letter` (under `(?i)`
/// Go does not case fold it, so it lacks U+0345), and the Unicode 17 scripts
/// `regex_syntax` does not know (`Beria_Erfe`, `Sidetic`, `Tai_Yo`,
/// `Tolong_Siki`). The sets still differ where Unicode 17 (Go) differs from
/// Unicode 16 (`regex_syntax`).
static UNICODE_CLASSES: &[(&str, &str)] = &[
    ("adlam", "sc=Adlam"),
    ("ahom", "sc=Ahom"),
    ("anatolianhieroglyphs", "sc=Anatolian_Hieroglyphs"),
    ("any", "Any"),
    ("arabic", "sc=Arabic"),
    ("armenian", "sc=Armenian"),
    ("ascii", "ASCII"),
    ("assigned", "Assigned"),
    ("avestan", "sc=Avestan"),
    ("balinese", "sc=Balinese"),
    ("bamum", "sc=Bamum"),
    ("bassavah", "sc=Bassa_Vah"),
    ("batak", "sc=Batak"),
    ("bengali", "sc=Bengali"),
    ("bhaiksuki", "sc=Bhaiksuki"),
    ("bopomofo", "sc=Bopomofo"),
    ("brahmi", "sc=Brahmi"),
    ("braille", "sc=Braille"),
    ("buginese", "sc=Buginese"),
    ("buhid", "sc=Buhid"),
    ("c", "gc=C"),
    ("canadianaboriginal", "sc=Canadian_Aboriginal"),
    ("carian", "sc=Carian"),
    ("caucasianalbanian", "sc=Caucasian_Albanian"),
    ("cc", "gc=Cc"),
    ("cf", "gc=Cf"),
    ("chakma", "sc=Chakma"),
    ("cham", "sc=Cham"),
    ("cherokee", "sc=Cherokee"),
    ("chorasmian", "sc=Chorasmian"),
    ("closepunctuation", "gc=Pe"),
    ("cn", "gc=Cn"),
    ("cntrl", "gc=Cc"),
    ("co", "gc=Co"),
    ("combiningmark", "gc=M"),
    ("common", "sc=Common"),
    ("connectorpunctuation", "gc=Pc"),
    ("control", "gc=Cc"),
    ("coptic", "sc=Coptic"),
    ("cuneiform", "sc=Cuneiform"),
    ("currencysymbol", "gc=Sc"),
    ("cypriot", "sc=Cypriot"),
    ("cyprominoan", "sc=Cypro_Minoan"),
    ("cyrillic", "sc=Cyrillic"),
    ("dashpunctuation", "gc=Pd"),
    ("decimalnumber", "gc=Nd"),
    ("deseret", "sc=Deseret"),
    ("devanagari", "sc=Devanagari"),
    ("digit", "gc=Nd"),
    ("divesakuru", "sc=Dives_Akuru"),
    ("dogra", "sc=Dogra"),
    ("duployan", "sc=Duployan"),
    ("egyptianhieroglyphs", "sc=Egyptian_Hieroglyphs"),
    ("elbasan", "sc=Elbasan"),
    ("elymaic", "sc=Elymaic"),
    ("enclosingmark", "gc=Me"),
    ("ethiopic", "sc=Ethiopic"),
    ("finalpunctuation", "gc=Pf"),
    ("format", "gc=Cf"),
    ("garay", "sc=Garay"),
    ("georgian", "sc=Georgian"),
    ("glagolitic", "sc=Glagolitic"),
    ("gothic", "sc=Gothic"),
    ("grantha", "sc=Grantha"),
    ("greek", "sc=Greek"),
    ("gujarati", "sc=Gujarati"),
    ("gunjalagondi", "sc=Gunjala_Gondi"),
    ("gurmukhi", "sc=Gurmukhi"),
    ("gurungkhema", "sc=Gurung_Khema"),
    ("han", "sc=Han"),
    ("hangul", "sc=Hangul"),
    ("hanifirohingya", "sc=Hanifi_Rohingya"),
    ("hanunoo", "sc=Hanunoo"),
    ("hatran", "sc=Hatran"),
    ("hebrew", "sc=Hebrew"),
    ("hiragana", "sc=Hiragana"),
    ("imperialaramaic", "sc=Imperial_Aramaic"),
    ("inherited", "sc=Inherited"),
    ("initialpunctuation", "gc=Pi"),
    ("inscriptionalpahlavi", "sc=Inscriptional_Pahlavi"),
    ("inscriptionalparthian", "sc=Inscriptional_Parthian"),
    ("javanese", "sc=Javanese"),
    ("kaithi", "sc=Kaithi"),
    ("kannada", "sc=Kannada"),
    ("katakana", "sc=Katakana"),
    ("kawi", "sc=Kawi"),
    ("kayahli", "sc=Kayah_Li"),
    ("kharoshthi", "sc=Kharoshthi"),
    ("khitansmallscript", "sc=Khitan_Small_Script"),
    ("khmer", "sc=Khmer"),
    ("khojki", "sc=Khojki"),
    ("khudawadi", "sc=Khudawadi"),
    ("kiratrai", "sc=Kirat_Rai"),
    ("l", "gc=L"),
    ("lao", "sc=Lao"),
    ("latin", "sc=Latin"),
    ("lepcha", "sc=Lepcha"),
    ("letter", "gc=L"),
    ("letternumber", "gc=Nl"),
    ("limbu", "sc=Limbu"),
    ("lineara", "sc=Linear_A"),
    ("linearb", "sc=Linear_B"),
    ("lineseparator", "gc=Zl"),
    ("lisu", "sc=Lisu"),
    ("ll", "gc=Ll"),
    ("lm", "gc=Lm"),
    ("lo", "gc=Lo"),
    ("lowercaseletter", "gc=Ll"),
    ("lt", "gc=Lt"),
    ("lu", "gc=Lu"),
    ("lycian", "sc=Lycian"),
    ("lydian", "sc=Lydian"),
    ("m", "gc=M"),
    ("mahajani", "sc=Mahajani"),
    ("makasar", "sc=Makasar"),
    ("malayalam", "sc=Malayalam"),
    ("mandaic", "sc=Mandaic"),
    ("manichaean", "sc=Manichaean"),
    ("marchen", "sc=Marchen"),
    ("mark", "gc=M"),
    ("masaramgondi", "sc=Masaram_Gondi"),
    ("mathsymbol", "gc=Sm"),
    ("mc", "gc=Mc"),
    ("me", "gc=Me"),
    ("medefaidrin", "sc=Medefaidrin"),
    ("meeteimayek", "sc=Meetei_Mayek"),
    ("mendekikakui", "sc=Mende_Kikakui"),
    ("meroiticcursive", "sc=Meroitic_Cursive"),
    ("meroitichieroglyphs", "sc=Meroitic_Hieroglyphs"),
    ("miao", "sc=Miao"),
    ("mn", "gc=Mn"),
    ("modi", "sc=Modi"),
    ("modifierletter", "gc=Lm"),
    ("modifiersymbol", "gc=Sk"),
    ("mongolian", "sc=Mongolian"),
    ("mro", "sc=Mro"),
    ("multani", "sc=Multani"),
    ("myanmar", "sc=Myanmar"),
    ("n", "gc=N"),
    ("nabataean", "sc=Nabataean"),
    ("nagmundari", "sc=Nag_Mundari"),
    ("nandinagari", "sc=Nandinagari"),
    ("nd", "gc=Nd"),
    ("newa", "sc=Newa"),
    ("newtailue", "sc=New_Tai_Lue"),
    ("nko", "sc=Nko"),
    ("nl", "gc=Nl"),
    ("no", "gc=No"),
    ("nonspacingmark", "gc=Mn"),
    ("number", "gc=N"),
    ("nushu", "sc=Nushu"),
    ("nyiakengpuachuehmong", "sc=Nyiakeng_Puachue_Hmong"),
    ("ogham", "sc=Ogham"),
    ("olchiki", "sc=Ol_Chiki"),
    ("oldhungarian", "sc=Old_Hungarian"),
    ("olditalic", "sc=Old_Italic"),
    ("oldnortharabian", "sc=Old_North_Arabian"),
    ("oldpermic", "sc=Old_Permic"),
    ("oldpersian", "sc=Old_Persian"),
    ("oldsogdian", "sc=Old_Sogdian"),
    ("oldsoutharabian", "sc=Old_South_Arabian"),
    ("oldturkic", "sc=Old_Turkic"),
    ("olduyghur", "sc=Old_Uyghur"),
    ("olonal", "sc=Ol_Onal"),
    ("openpunctuation", "gc=Ps"),
    ("oriya", "sc=Oriya"),
    ("osage", "sc=Osage"),
    ("osmanya", "sc=Osmanya"),
    ("other", "gc=C"),
    ("otherletter", "gc=Lo"),
    ("othernumber", "gc=No"),
    ("otherpunctuation", "gc=Po"),
    ("othersymbol", "gc=So"),
    ("p", "gc=P"),
    ("pahawhhmong", "sc=Pahawh_Hmong"),
    ("palmyrene", "sc=Palmyrene"),
    ("paragraphseparator", "gc=Zp"),
    ("paucinhau", "sc=Pau_Cin_Hau"),
    ("pc", "gc=Pc"),
    ("pd", "gc=Pd"),
    ("pe", "gc=Pe"),
    ("pf", "gc=Pf"),
    ("phagspa", "sc=Phags_Pa"),
    ("phoenician", "sc=Phoenician"),
    ("pi", "gc=Pi"),
    ("po", "gc=Po"),
    ("privateuse", "gc=Co"),
    ("ps", "gc=Ps"),
    ("psalterpahlavi", "sc=Psalter_Pahlavi"),
    ("punct", "gc=P"),
    ("punctuation", "gc=P"),
    ("rejang", "sc=Rejang"),
    ("runic", "sc=Runic"),
    ("s", "gc=S"),
    ("samaritan", "sc=Samaritan"),
    ("saurashtra", "sc=Saurashtra"),
    ("sc", "gc=Sc"),
    ("separator", "gc=Z"),
    ("sharada", "sc=Sharada"),
    ("shavian", "sc=Shavian"),
    ("siddham", "sc=Siddham"),
    ("signwriting", "sc=SignWriting"),
    ("sinhala", "sc=Sinhala"),
    ("sk", "gc=Sk"),
    ("sm", "gc=Sm"),
    ("so", "gc=So"),
    ("sogdian", "sc=Sogdian"),
    ("sorasompeng", "sc=Sora_Sompeng"),
    ("soyombo", "sc=Soyombo"),
    ("spaceseparator", "gc=Zs"),
    ("spacingmark", "gc=Mc"),
    ("sundanese", "sc=Sundanese"),
    ("sunuwar", "sc=Sunuwar"),
    ("sylotinagri", "sc=Syloti_Nagri"),
    ("symbol", "gc=S"),
    ("syriac", "sc=Syriac"),
    ("tagalog", "sc=Tagalog"),
    ("tagbanwa", "sc=Tagbanwa"),
    ("taile", "sc=Tai_Le"),
    ("taitham", "sc=Tai_Tham"),
    ("taiviet", "sc=Tai_Viet"),
    ("takri", "sc=Takri"),
    ("tamil", "sc=Tamil"),
    ("tangsa", "sc=Tangsa"),
    ("tangut", "sc=Tangut"),
    ("telugu", "sc=Telugu"),
    ("thaana", "sc=Thaana"),
    ("thai", "sc=Thai"),
    ("tibetan", "sc=Tibetan"),
    ("tifinagh", "sc=Tifinagh"),
    ("tirhuta", "sc=Tirhuta"),
    ("titlecaseletter", "gc=Lt"),
    ("todhri", "sc=Todhri"),
    ("toto", "sc=Toto"),
    ("tulutigalari", "sc=Tulu_Tigalari"),
    ("ugaritic", "sc=Ugaritic"),
    ("unassigned", "gc=Cn"),
    ("uppercaseletter", "gc=Lu"),
    ("vai", "sc=Vai"),
    ("vithkuqi", "sc=Vithkuqi"),
    ("wancho", "sc=Wancho"),
    ("warangciti", "sc=Warang_Citi"),
    ("yezidi", "sc=Yezidi"),
    ("yi", "sc=Yi"),
    ("z", "gc=Z"),
    ("zanabazarsquare", "sc=Zanabazar_Square"),
    ("zl", "gc=Zl"),
    ("zp", "gc=Zp"),
    ("zs", "gc=Zs"),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn compile(raw: &str) -> Result<regex::Regex, String> {
        compile_literal(raw)
    }

    fn match_bits(raw: &str, haystacks: &[&str]) -> String {
        let re = compile(raw).unwrap_or_else(|err| panic!("{raw}: {err}"));
        haystacks
            .iter()
            .map(|h| if re.is_match(h) { '1' } else { '0' })
            .collect()
    }

    #[test]
    fn matches_go_on_differential_corpus() {
        let mut failures = Vec::new();
        for &(raw, want) in GO_CASES {
            match (compile(raw), want) {
                (Ok(re), Some(bits)) => {
                    let got: String = HAYSTACKS
                        .iter()
                        .map(|h| if re.is_match(h) { '1' } else { '0' })
                        .collect();
                    if got != bits {
                        failures.push(format!("{raw}: matches {got}, Go {bits}"));
                    }
                }
                (Err(_), None) => {}
                (got, want) => failures.push(format!(
                    "{raw}: accepted {}, Go accepted {}",
                    got.is_ok(),
                    want.is_some()
                )),
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// Go 1.27.1 results. Perl and POSIX classes are ASCII; under `(?i)` Go
    /// case folds them before negating, so U+212A KELVIN SIGN folds to k and
    /// U+017F LATIN SMALL LETTER LONG S to s.
    #[test]
    fn perl_classes_are_ascii_and_case_fold_like_go() {
        let haystacks = [
            "\u{212A}", "\u{17F}", "k", "s", "K", "S", "\u{E9}", "_", "0", " ",
        ];
        let cases = [
            (r"/\w/i", "1111110110"),
            (r"/(?i)\w/", "1111110110"),
            (r"/[\w]/i", "1111110110"),
            (r"/[[:word:]]/i", "1111110110"),
            (r"/[[:alpha:]]/i", "1111110000"),
            (r"/[a-z]/i", "1111110000"),
            (r"/k/i", "1010100000"),
            (r"/s/i", "0101010000"),
            (r"/\w/", "0011110110"),
            (r"/\W/i", "0000001001"),
            (r"/[^\w]/i", "0000001001"),
            (r"/[\W]/i", "0000001001"),
            (r"/[^\W]/i", "1111110110"),
            (r"/\W/", "1100001001"),
            (r"/[\W]/", "1100001001"),
            (r"/\s/", "0000000001"),
            (r"/\S/", "1111111110"),
            (r"/[\S]/i", "1111111110"),
            (r"/\d/", "0000000010"),
            (r"/[\D]/i", "1111111101"),
        ];
        for (raw, want) in cases {
            assert_eq!(match_bits(raw, &haystacks), want, "{raw}");
        }
    }

    /// Go 1.27.1 results: `\b` and `\B` use ASCII word characters and only
    /// hold between characters.
    #[test]
    fn ascii_word_boundaries_hold_only_between_characters() {
        let haystacks = [
            "a\u{212A}b",
            "\u{212A}",
            "\u{E9}",
            "a\u{E9}",
            "\u{E9}\u{E9}",
            "ab",
            "a",
        ];
        let cases = [
            (r"/\B|\x{212A}\b/", "1111110"),
            (r"/\B/", "0111110"),
            (r"/\x{212A}\B/", "0100000"),
            (r"/\B\x{212A}/", "0100000"),
            (r"/(?:\B|a)b/", "0000010"),
            (r"/\b/", "1001011"),
            (r"/\b\x{E9}/", "0001000"),
            (r"/a\b/", "1001001"),
        ];
        for (raw, want) in cases {
            assert_eq!(match_bits(raw, &haystacks), want, "{raw}");
        }
    }

    /// Patterns both Go 1.27.1 and this module accept.
    #[test]
    fn accepts_what_go_accepts() {
        for raw in [
            r"/(a{2}){500}/",
            r"/(?:a{10}){100}/",
            r"/(?:(?:a{3}){3}){111}/",
            r"/(?:a{2,}){500}/",
            r"/(?:(?:a{3}){0}){999}/",
            r"/(?:a{0}){1000}/",
            r"/(?:a{30}|b{40}){25}/",
            r"/(?:a*){1000}/",
            r"/(?:a{1,}){1000}/",
            r"/a{1000}/",
            r"/a{0,1000}/",
            r"/a{1000,}/",
            r"/(?P<A_9>a)/",
            r"/(?<n>a)/",
            r"/(?iU)a+/",
            r"/(?i-s:a)/",
            r"/\x{10FFFF}/",
            r"/\_/",
            r"/\ /",
            r"/\-/",
            r"/[\-]/",
            r"/\p{greek}/",
            r"/\p{OLD_italic}/",
            r"/\p{ Greek }/",
            r"/\p{Lowercase-Letter}/",
            r"/\p{any}/",
            r"/\pl/",
            r"/\P{assigned}/",
            r"/[]a]/",
            r"/[^]a]/",
            r"/[]-]/",
            r"/\pL{300}/",
            r"/[\x00-\x{10FFFF}]/",
            r"/[[:^word:]]/i",
            r"/^*/",
            r"/(?:)*/",
            r"/a{0}/",
            &format!("/{}a{}/", "(".repeat(50), ")".repeat(50)),
            &format!("/{}(?i)a{}/", "(?:".repeat(49), ")".repeat(49)),
        ] {
            assert!(compile(raw).is_ok(), "{raw}: {:?}", compile(raw).err());
        }
    }

    /// Patterns both Go 1.27.1 and this module reject, mostly syntax only
    /// `regex_syntax` knows.
    #[test]
    fn rejects_what_go_rejects() {
        for raw in [
            r"/(?x)a/",
            r"/(?u)a/",
            r"/(?R)a/",
            r"/(?i-x:a)/",
            r"/[a&&b]/",
            r"/[a--b]/",
            r"/[a~~b]/",
            r"/[a[b]]/",
            r"/\u0041/",
            r"/\U00000041/",
            r"/\u{41}/",
            r"/[\u0041]/",
            r"/\<a/",
            r"/a\>/",
            r"/\b{start}a/",
            r"/a**/",
            r"/a+*/",
            r"/a*?*/",
            r"/a{2}{3}/",
            r"/a{2}*/",
            r"/a{1001}/",
            r"/a{0,1001}/",
            r"/a{1001,}/",
            r"/(a{2}){501}/",
            r"/(?:a{10}){101}/",
            r"/(?:(?:a{3}){3}){112}/",
            r"/(?:a{2,}){501}/",
            r"/(?:(?:a{30}){40}){0,0}/",
            r"/(?P<a.b>x)/",
            r"/(?P<é>x)/",
            r"/(?P<a[1]>x)/",
            r"/\p{IsGreek}/",
            r"/\p{sc=Greek}/",
            r"/\p{Grek}/",
            r"/\p{Alphabetic}/",
            r"/\p{White_Space}/",
            r"/\p{Gréek}/",
            r"/\p{L&}/",
            r"/\p{}/",
            r"/[[:foo:]]/",
            r"/\pé/",
            r"/\Z/",
            r"/\C/",
            r"/\e/",
            r"/\h/",
            r"/(?=a)/",
            r"/(?#c)/",
            r"/(?P=n)/",
            r"/\k<n>/",
            r"/\x{110000}/",
            r"/(?-)a/",
            r"/(?i-)a/",
            r"/a{3,2}/",
            r"/[a-\d]/",
            r"/[z-a]/",
        ] {
            assert!(compile(raw).is_err(), "{raw}");
        }
    }

    /// Patterns Go 1.27.1 accepts that this module rejects: Go reads them
    /// differently from `regex_syntax`, `regex_syntax` cannot parse them, or
    /// they exceed a limit.
    #[test]
    fn rejects_forms_the_engines_disagree_on() {
        for raw in [
            r"/(?)a/",
            r"/(?ii)a/",
            r"/(?i-i)a/",
            r"/a(?i)*/",
            r"/(?P<1>a)/",
            r"/(?P<n>a)(?P<n>b)/",
            r"/\x{D800}/",
            r"/[\pL-\pN]/",
            r"/a{01}/",
            r"/a{1,02}/",
            r"/a{00}/",
            r"/[]-a]/",
            r"/[^]-a]/",
            r"/\p{LC}/",
            r"/\p{Cased_Letter}/",
            r"/\p{Cs}/",
            r"/\p{Surrogate}/",
            r"/\p{Tai_Yo}/",
            r"/\p{Beria_Erfe}/",
            r"/\p{Sidetic}/",
            r"/\p{Tolong_Siki}/",
            &format!("/{}a{}/", "(".repeat(51), ")".repeat(51)),
            &format!("/{}(?i)a{}/", "(?:".repeat(50), ")".repeat(50)),
        ] {
            assert!(compile(raw).is_err(), "{raw}");
        }
    }

    #[test]
    fn rewrites_classes_for_the_regex_crate() {
        let cases = [
            (
                r"\d\D\w\W\s\S",
                r"[0-9][^0-9][0-9A-Za-z_][^0-9A-Za-z_][\t\n\f\r ][^\t\n\f\r ]",
            ),
            (r"[\d\D\w\W]", "[[:digit:][:^digit:][:word:][:^word:]]"),
            (r"[\s\S]", r"[\t\n\f\r \x00-\x08\x0B\x0E-\x1F!-\x{10FFFF}]"),
            (r"a\bb\B", r"a(?-u:\b)b(?:(?-u:\B)(?:\b|\B))"),
            (
                r"\pL\P{greek}[\p{Old Italic}]",
                r"\p{gc=L}\P{sc=Greek}[\p{sc=Old_Italic}]",
            ),
            (r"(?i)a{2}", r"(?i)a{2}"),
        ];
        for (pattern, want) in cases {
            assert_eq!(rewrite(pattern).unwrap(), want, "{pattern}");
        }
    }

    #[test]
    fn unicode_class_names_follow_go_spelling() {
        assert!(UNICODE_CLASSES.windows(2).all(|w| w[0].0 < w[1].0));
        assert_eq!(UNICODE_CLASSES.len(), 249);
        assert_eq!(unicode_class_target("Old_Italic"), Some("sc=Old_Italic"));
        assert_eq!(unicode_class_target("old italic"), Some("sc=Old_Italic"));
        assert_eq!(unicode_class_target("-OLD-ITALIC-"), Some("sc=Old_Italic"));
        assert_eq!(unicode_class_target("lu"), Some("gc=Lu"));
        assert_eq!(unicode_class_target("Uppercase_Letter"), Some("gc=Lu"));
        assert_eq!(unicode_class_target("digit"), Some("gc=Nd"));
        assert_eq!(unicode_class_target("ascii"), Some("ASCII"));
        assert_eq!(unicode_class_target("Greek\u{A0}"), None);
        assert_eq!(unicode_class_target("Gre\tek"), None);
        assert_eq!(unicode_class_target("IsGreek"), None);
    }

    #[test]
    fn literal_pattern_ports_parse_regex() {
        assert_eq!(literal_pattern("/a/").unwrap(), "a");
        assert_eq!(literal_pattern("|a/b|i").unwrap(), "(?i)a/b");
        assert_eq!(literal_pattern("/a|b/ms").unwrap(), "(?ms)a|b");
        assert_eq!(literal_pattern("/a/smi").unwrap(), "(?smi)a");
        assert_eq!(
            literal_pattern("/a/imi").unwrap_err(),
            "duplicate regex flag 'i'"
        );
        assert_eq!(
            literal_pattern("/a/mss").unwrap_err(),
            "duplicate regex flag 's'"
        );
        assert!(literal_pattern("").is_err());
        assert!(literal_pattern("/").is_err());
    }

    #[test]
    fn dialect_errors_match_go() {
        let cases = [
            (r"\<a", r"\< is not supported; use \b for word boundaries"),
            (
                r"\Qa",
                r"\Q...\E is not supported; escape each character instead",
            ),
            (
                r"\0",
                r"\0 is not supported; use \x{...} for character codes",
            ),
            (r"\B{x}", r"\B{...} is not supported"),
            (
                r"\P{^L}",
                r"\P{^...} is not supported; use \P{...} to negate a Unicode class",
            ),
            (
                r"[\w-z]",
                r"\w cannot start a range in a character class; escape the dash as \-",
            ),
            (
                r"[\p{Lu}-z]",
                r"\p cannot start a range in a character class; escape the dash as \-",
            ),
            (
                "[a[b]",
                r"nested character classes are not supported; escape [ as \[",
            ),
            (
                "[a&&b]",
                r"&& in a character class is not supported; escape it as \&\&",
            ),
            ("a{,2}", "{,n} is not supported; use {0,n}"),
            (
                "a{x",
                r"{ must start a repetition such as {2} or {1,3}; escape a literal brace as \{",
            ),
        ];
        for (pattern, want) in cases {
            assert_eq!(check_regex_dialect(pattern).unwrap_err(), want, "{pattern}");
        }
        for pattern in [
            r"a\",
            r"[\d-]",
            r"[\d\-z]",
            "[[:alpha:]]",
            "[]a]",
            "[^]a]",
            r"\x{41}",
            "a{2,}",
        ] {
            assert_eq!(check_regex_dialect(pattern), Ok(()), "{pattern}");
        }
    }

    /// Haystacks for [`GO_CASES`] (no code points whose properties differ
    /// between Unicode 16 and 17).
    const HAYSTACKS: &[&str] = &[
        "",
        "a",
        "A",
        "b",
        "ab",
        "AB",
        "abc",
        "aa",
        "x",
        "K",
        "k",
        "\u{212a}",
        "s",
        "S",
        "\u{17f}",
        "\n",
        "a\nb",
        "x\na\nb",
        "a\n",
        "\u{b}",
        "\t",
        " ",
        "\r\n",
        "\u{a0}",
        "\u{2028}",
        "\u{e9}",
        "a\u{e9}",
        "\u{e9}a",
        "\u{3b1}\u{3b2}\u{3b3}",
        "\u{3a3}",
        "\u{3c2}",
        "\u{65e5}\u{672c}",
        "\u{1f600}",
        "0123",
        "\u{663}",
        "foo_bar",
        "a{01}",
        "{",
        "-",
        "a-b",
        "]",
        r"\",
        "_",
        "CURL/8.0",
        "/some/path/here",
        "a.b",
        "\u{0}",
        "\u{10ffff}",
        "\u{df}",
        "\u{1e9e}",
        "\u{2126}",
        "\u{3c9}",
        "\u{1c5}",
        "\u{1c6}",
        "\u{378}",
        "\u{e000}",
        "stra\u{df}e",
        "STRASSE",
        "\u{212a}\u{212a}",
        "a\u{212a}b",
        "\u{130}",
        "\u{131}",
        "\u{24b6}",
        "\u{24d0}",
        "\u{345}",
        "\u{3b9}",
        "\u{1fbe}",
        "\u{c}",
        "\r",
        "a b",
        "a_b",
        "\u{300}",
        "a\u{300}",
        "\u{3000}",
        "\u{85}",
        "\u{b2}",
    ];

    /// Go 1.27.1 `parseRegex` results: the `MatchString` result for each
    /// haystack (1 = match), or `None` when Go rejects the pattern.
    const GO_CASES: &[(&str, Option<&str>)] = &[
        (
            "/curl/i",
            Some("0000000000000000000000000000000000000000000100000000000000000000000000000000"),
        ),
        (
            r"/example\.com$/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "|^/usr/bin/|",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/^x/",
            Some("0000000010000000010000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/^d/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/^z/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/^10\./",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/^https:/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/^192\./",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/^01:23/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/x/i",
            Some("0000000010000000010000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/x/",
            Some("0000000010000000010000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/gl=se$/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "|some/path/here|",
            Some("0000000000000000000000000000000000000000000010000000000000000000000000000000"),
        ),
        (
            "|curl|i",
            Some("0000000000000000000000000000000000000000000100000000000000000000000000000000"),
        ),
        (
            "/curl/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/a.b/s",
            Some("0000000000000000110000000000000000000001000001000000000000010000000001100000"),
        ),
        (
            "/^b$/m",
            Some("0001000000000000110000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/^A.B$/ims",
            Some("0000000000000000110000000000000000000001000001000000000000010000000001100000"),
        ),
        (r"/\<a/", None),
        (r"/a\>/", None),
        (r"/\Qa.b\E/", None),
        (r"/\0/", None),
        (r"/\12/", None),
        (r"/\b{start}a/", None),
        (r"/\p{^L}/", None),
        ("/a{,3}/", None),
        (r"/[\d-z]/", None),
        (r"/[\p{L}-z]/", None),
        ("/[a[b]]/", None),
        ("/[[a]]/", None),
        ("/[a-z&&b]/", None),
        ("/[a--b]/", None),
        ("/[a~~b]/", None),
        (
            r"/\ba/",
            Some("0100101100000000111000000011000000001001000001000000000000010000000001101000"),
        ),
        (
            r"/\x{1F600}/",
            Some("0000000000000000000000000000000010000000000000000000000000000000000000000000"),
        ),
        (
            "/a{0,3}/",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            r"/\P{L}/",
            Some("0000000000000001111111111000000011111111111111110000001100000011100111111111"),
        ),
        (
            r"/[\d\-z]/",
            Some("0000000000000000000000000000000001001011000100000000000000000000000000000000"),
        ),
        (
            "/[a-z]/",
            Some("0101101110101000111000000011000000011001000011000000000010010000000001101000"),
        ),
        (
            "/[[:alpha:]]/",
            Some("0111111111101100111000000011000000011001000111000000000011010000000001101000"),
        ),
        (
            "/[[:^digit:]x]/",
            Some("0111111111111111111111111111111110111111111111111111111111111111111111111111"),
        ),
        (
            r"/[\[\]]/",
            Some("0000000000000000000000000000000000000000100000000000000000000000000000000000"),
        ),
        (
            "/[]a]/",
            Some("0100101100000000111000000011000000011001100011000000000010010000000001101000"),
        ),
        (
            "/[^]a]/",
            Some("0011111011111111111111111111111111111111011111111111111111111111111111111111"),
        ),
        (
            "/[a-]/",
            Some("0100101100000000111000000011000000011011000011000000000010010000000001101000"),
        ),
        (
            "/[a&b~c-]/",
            Some("0101101100000000111000000011000000011011000011000000000010010000000001101000"),
        ),
        (
            r"/\{,3\}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/a{2}/",
            Some("0000000100000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/a{2,}/",
            Some("0000000100000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/a\{x\}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/[{]/",
            Some("0000000000000000000000000000000000001100000000000000000000000000000000000000"),
        ),
        ("/a/ii", None),
        ("/a{x}/", None),
        ("/a{1,2/", None),
        ("/{/", None),
        (
            r"/\w/i",
            Some("0111111111111110111000000011000001011001001111000000000011110000000001101000"),
        ),
        (
            r"/(?i)\w/",
            Some("0111111111111110111000000011000001011001001111000000000011110000000001101000"),
        ),
        (
            r"/[\w]/i",
            Some("0111111111111110111000000011000001011001001111000000000011110000000001101000"),
        ),
        (
            r"/\W/i",
            Some("0000000000000001111111111111111110101111110111111111111110001111111111011111"),
        ),
        (
            r"/[^\w]/i",
            Some("0000000000000001111111111111111110101111110111111111111110001111111111011111"),
        ),
        (
            r"/[\W]/i",
            Some("0000000000000001111111111111111110101111110111111111111110001111111111011111"),
        ),
        (
            r"/[^\W]/i",
            Some("0111111111111110111000000011000001011001001111000000000011110000000001101000"),
        ),
        (
            "/k/i",
            Some("0000000001110000000000000000000000000000000000000000000000110000000000000000"),
        ),
        (
            "/K/i",
            Some("0000000001110000000000000000000000000000000000000000000000110000000000000000"),
        ),
        (
            "/s/i",
            Some("0000000000001110000000000000000000000000000010000000000011000000000000000000"),
        ),
        (
            r"/\x{212A}/i",
            Some("0000000001110000000000000000000000000000000000000000000000110000000000000000"),
        ),
        (
            "/[a-z]/i",
            Some("0111111111111110111000000011000000011001000111000000000011110000000001101000"),
        ),
        (
            "/[a-z]+/i",
            Some("0111111111111110111000000011000000011001000111000000000011110000000001101000"),
        ),
        (
            "/[[:upper:]]/i",
            Some("0111111111111110111000000011000000011001000111000000000011110000000001101000"),
        ),
        (
            "/[[:^upper:]]/i",
            Some("0000000000000001111111111111111111111111111111111111111110001111111111111111"),
        ),
        (
            "/[[:word:]]/i",
            Some("0111111111111110111000000011000001011001001111000000000011110000000001101000"),
        ),
        (
            "/[[:lower:]]/",
            Some("0101101110101000111000000011000000011001000011000000000010010000000001101000"),
        ),
        (
            r"/\pL/i",
            Some("0111111111111110111000000111111100011001000111001111110011111100111001101000"),
        ),
        (
            r"/\p{Lu}/i",
            Some("0111111111111110111000000111111000011001000111001111110011111000111001101000"),
        ),
        (
            r"/\p{Lu}/",
            Some("0010010001010100000000000000010000000000000100000110000001111000000000000000"),
        ),
        (
            r"/\P{Lu}/i",
            Some("0000000000000001111111111000000111111111111111110000001100000111000111111111"),
        ),
        (
            r"/[^\p{Lu}]/i",
            Some("0000000000000001111111111000000111111111111111110000001100000111000111111111"),
        ),
        (
            r"/\p{Greek}/i",
            Some("0000000000000000000000000000111000000000000000000011000000000000111000000000"),
        ),
        (
            r"/\p{Greek}/",
            Some("0000000000000000000000000000111000000000000000000011000000000000011000000000"),
        ),
        (
            r"/\p{greek}/",
            Some("0000000000000000000000000000111000000000000000000011000000000000011000000000"),
        ),
        (
            r"/\p{GREEK}/",
            Some("0000000000000000000000000000111000000000000000000011000000000000011000000000"),
        ),
        (
            r"/\p{Old_Italic}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/\p{OldItalic}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/\p{old italic}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/\p{Letter}/",
            Some("0111111111111110111000000111111100011001000111001111110011111100011001101000"),
        ),
        (
            r"/\p{Uppercase_Letter}/",
            Some("0010010001010100000000000000010000000000000100000110000001111000000000000000"),
        ),
        (
            r"/\p{Uppercase Letter}/",
            Some("0010010001010100000000000000010000000000000100000110000001111000000000000000"),
        ),
        (
            r"/\p{uppercase-letter}/",
            Some("0010010001010100000000000000010000000000000100000110000001111000000000000000"),
        ),
        (r"/\p{L&}/", None),
        (
            r"/\p{Any}/",
            Some("0111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            r"/\p{Assigned}/",
            Some("0111111111111111111111111111111111111111111111101111110111111111111111111111"),
        ),
        (
            r"/\P{Assigned}/",
            Some("0000000000000000000000000000000000000000000000010000001000000000000000000000"),
        ),
        (
            r"/\p{ASCII}/",
            Some("0111111111101101111111100011000001011111111111100000000011010000000111101000"),
        ),
        (
            r"/\p{Ascii}/i",
            Some("0111111111111111111111100011000001011111111111100000000011110000000111101000"),
        ),
        (
            r"/\p{Cn}/",
            Some("0000000000000000000000000000000000000000000000010000001000000000000000000000"),
        ),
        (
            r"/\pC/",
            Some("0000000000000001111110100000000000000000000000110000001100000000000110000010"),
        ),
        (
            r"/\p{C}/",
            Some("0000000000000001111110100000000000000000000000110000001100000000000110000010"),
        ),
        (
            r"/\p{Sc}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (r"/\p{sc=Greek}/", None),
        (r"/\p{gc=Lu}/", None),
        (r"/\p{Script=Greek}/", None),
        (r"/\p{Alphabetic}/", None),
        (r"/\p{Emoji}/", None),
        (
            r"/\p{Han}/",
            Some("0000000000000000000000000000000100000000000000000000000000000000000000000000"),
        ),
        (
            r"/\pN/",
            Some("0000000000000000000000000000000001101000000100000000000000000000000000000001"),
        ),
        (
            r"/\pl/",
            Some("0111111111111110111000000111111100011001000111001111110011111100011001101000"),
        ),
        (
            r"/\pL/",
            Some("0111111111111110111000000111111100011001000111001111110011111100011001101000"),
        ),
        (
            r"/\PL/",
            Some("0000000000000001111111111000000011111111111111110000001100000011100111111111"),
        ),
        (r"/\p{IsGreek}/", None),
        (r"/\p{InGreek}/", None),
        (
            r"/\p{Common}/",
            Some("0000000000000001111111111000000011011111111111100000000000000011000111100111"),
        ),
        (r"/\p{Zyyy}/", None),
        (r"/\p{Latn}/", None),
        (
            r"/\p{Latin}/i",
            Some("0111111111111110111000000111000000011001000111001100110011111100000001101000"),
        ),
        (
            r"/\p{Cherokee}/i",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/\p{Lt}/i",
            Some("0000000000000000000000000000000000000000000000000000110000000000000000000000"),
        ),
        (
            r"/\p{Ll}/i",
            Some("0111111111111110111000000111111000011001000111001111110011110100111001101000"),
        ),
        (r"/\p/", None),
        (r"/\p{}/", None),
        (r"/\p{L/", None),
        ("/\\p\u{e9}/", None),
        (r"/\p^L/", None),
        (r"/\P/", None),
        (
            r"/\s/",
            Some("0000000000000001111011100000000000000000000000000000000000000000000111000000"),
        ),
        (
            r"/[\s]/",
            Some("0000000000000001111011100000000000000000000000000000000000000000000111000000"),
        ),
        (
            r"/\S/",
            Some("0111111111111110111100011111111111111111111111111111111111111111111001111111"),
        ),
        (
            r"/[\S]/",
            Some("0111111111111110111100011111111111111111111111111111111111111111111001111111"),
        ),
        (
            "/[[:space:]]/",
            Some("0000000000000001111111100000000000000000000000000000000000000000000111000000"),
        ),
        (
            "/[[:blank:]]/",
            Some("0000000000000000000011000000000000000000000000000000000000000000000001000000"),
        ),
        (
            r"/\v/",
            Some("0000000000000000000100000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/[\v]/",
            Some("0000000000000000000100000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/\x0b/",
            Some("0000000000000000000100000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/\\b\u{e9}/",
            Some("0000000000000000000000000010000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/a\b/",
            Some("0100000100000000111000000011000000001001000001000000000010010000000001001000"),
        ),
        (
            "/\\B\u{e9}/",
            Some("0000000000000000000000000101000000000000000000000000000000000000000000000000"),
        ),
        (
            "/\u{e9}\\b/",
            Some("0000000000000000000000000001000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/\b/",
            Some("0111111111101100111000000011000001011001001111000000000011010000000001101000"),
        ),
        (
            r"/\B/",
            Some("1000111100010011001111111111111111111110110110111111111111101111111110111111"),
        ),
        (
            r"/a\B/",
            Some("0000101100000000000000000000000000010000000010000000000000000000000000100000"),
        ),
        (
            r"/\bfoo\b/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        ("/a**/", None),
        ("/a*?*/", None),
        ("/a*??/", None),
        ("/a+*/", None),
        ("/a{2}{3}/", None),
        ("/a*{2}/", None),
        ("/a**?/", None),
        (
            "/(?:a*)*/",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/(a*)*/",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        ("/(?i)*/", None),
        ("/*/", None),
        ("/a|*/", None),
        ("/(*)/", None),
        (
            "/^*/",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/$*/",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            r"/\b*/",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/(?:)*/",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/a{2}?/",
            Some("0000000100000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        ("/a{2}?{3}/", None),
        (
            "/a??/",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/a+?/",
            Some("0100101100000000111000000011000000011001000011000000000010010000000001101000"),
        ),
        (
            "/x{2,3}?/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/a{1000}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        ("/a{1001}/", None),
        (
            "/a{0,1000}/",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        ("/a{0,1001}/", None),
        (
            "/a{1000,}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        ("/a{1001,}/", None),
        (
            "/(?:a{2}){500}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        ("/(?:a{2}){501}/", None),
        (
            "/(?:a{10}){100}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        ("/(?:a{10}){101}/", None),
        (
            "/((a{10}){10}){10}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        ("/((a{10}){10}){11}/", None),
        (
            "/(?:a{1000})*/",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/(?:a{1000}){1}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/(?:a{1000}){0,1}/",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/(?:a{1000}){1,}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        ("/(?:(?:a{1000}){1,}){2}/", None),
        (
            "/(?:a{1000}){0}/",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        ("/(?:a{2}|b{3}){400}/", None),
        (
            "/(?:a{2}|b{3}){300}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        ("/a{2,1}/", None),
        (
            "/a{0}/",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        ("/a{99999999999}/", None),
        ("/a{1,99999999999}/", None),
        ("/a{100000000}/", None),
        ("/a{99999999}/", None),
        ("/{2}/", None),
        (
            r"/\pL{1000}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/(?:\pL{100}){10}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/[0-9]{1000}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/(?i)a/",
            Some("0110111100000000111000000011000000011001000011000000000011010000000001101000"),
        ),
        (
            "/(?i:a)b/",
            Some("0000101000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/(?-i)a/i",
            Some("0100101100000000111000000011000000011001000011000000000010010000000001101000"),
        ),
        ("/(?-)a/", None),
        ("/(?i-)a/", None),
        ("/(?--i)a/", None),
        ("/(?i-s-m)a/", None),
        ("/(?x)a/", None),
        ("/(?u)a/", None),
        ("/(?R)a/", None),
        (
            "/(?U)a+/",
            Some("0100101100000000111000000011000000011001000011000000000010010000000001101000"),
        ),
        (
            "/(?s)./",
            Some("0111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/(?m)^b/",
            Some("0001000000000000110000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/(?sm:^.)/",
            Some("0111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/(?P<name>a)/",
            Some("0100101100000000111000000011000000011001000011000000000010010000000001101000"),
        ),
        (
            "/(?<name>a)/",
            Some("0100101100000000111000000011000000011001000011000000000010010000000001101000"),
        ),
        ("/(?P<n-m>a)/", None),
        ("/(?P<>a)/", None),
        ("/(?P=name)/", None),
        ("/(?'n'a)/", None),
        ("/(?#comment)/", None),
        ("/(?=a)/", None),
        ("/(?!a)/", None),
        ("/(?<=a)b/", None),
        ("/(?<!a)b/", None),
        ("/(?>a)/", None),
        ("/(?i/", None),
        ("/(?/", None),
        ("/(?P<na/", None),
        ("/(?P<\u{e9}>a)/", None),
        (r"/\Qa\E/", None),
        (
            r"/\\Q/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/\x41/",
            Some("0010010000000000000000000000000000000000000000000000000001000000000000000000"),
        ),
        (r"/\x4/", None),
        (r"/\x{}/", None),
        (r"/\x{110000}/", None),
        (
            r"/\x{10FFFF}/",
            Some("0000000000000000000000000000000000000000000000010000000000000000000000000000"),
        ),
        (
            r"/[\x{D7FF}-\x{E000}]/",
            Some("0000000000000000000000000000000000000000000000000000000100000000000000000000"),
        ),
        (r"/\x{zz}/", None),
        (r"/\x{41/", None),
        (r"/\xg1/", None),
        (
            r"/[^\x{0}-\x{10FFFF}]/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/\x{0000000041}/",
            Some("0010010000000000000000000000000000000000000000000000000001000000000000000000"),
        ),
        (r"/\x/", None),
        (r"/\u0041/", None),
        (r"/\U00000041/", None),
        (r"/\e/", None),
        (
            r"/\a/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/\f/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000100000000"),
        ),
        (
            r"/\t/",
            Some("0000000000000000000010000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/\n/",
            Some("0000000000000001111000100000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/\r/",
            Some("0000000000000000000000100000000000000000000000000000000000000000000010000000"),
        ),
        (
            r"/\_/",
            Some("0000000000000000000000000000000000010000001000000000000000000000000000100000"),
        ),
        (
            r"/\-/",
            Some("0000000000000000000000000000000000000011000000000000000000000000000000000000"),
        ),
        (
            r"/\ /",
            Some("0000000000000000000001000000000000000000000000000000000000000000000001000000"),
        ),
        (
            r"/\#/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/\~/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/\@/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/\%/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/\'/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/\"/"#,
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        ("/\\\u{e9}/", None),
        (r"/\c/", None),
        (r"/\k/", None),
        (r"/\g/", None),
        (r"/\h/", None),
        (r"/\K/", None),
        (r"/\G/", None),
        (r"/\X/", None),
        (r"/\R/", None),
        (r"/\N/", None),
        (r"/\o{101}/", None),
        (r"/\Z/", None),
        (
            r"/\z/",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            r"/\A/",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (r"/\C/", None),
        (r"/a\/", None),
        (r"/\012/", None),
        (r"/\8/", None),
        (
            r"/a\z/",
            Some("0100000100000000000000000001000000000000000000000000000000000000000000000000"),
        ),
        (
            "/a$/",
            Some("0100000100000000000000000001000000000000000000000000000000000000000000000000"),
        ),
        (
            "/a$/m",
            Some("0100000100000000111000000001000000000000000000000000000000000000000000000000"),
        ),
        (
            "/^a/m",
            Some("0100101100000000111000000010000000001001000001000000000000010000000001101000"),
        ),
        (
            r"/\Aa/m",
            Some("0100101100000000101000000010000000001001000001000000000000010000000001101000"),
        ),
        (
            r"/a\z/m",
            Some("0100000100000000000000000001000000000000000000000000000000000000000000000000"),
        ),
        (
            "/a|/",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/|a/",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/|/",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/a||b/",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/()/",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/(|)/",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/(?:)/",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "//",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/(a|b)|c/",
            Some("0101101100000000111000000011000000011001000011000000000010010000000001101000"),
        ),
        (
            "/a|b|c/",
            Some("0101101100000000111000000011000000011001000011000000000010010000000001101000"),
        ),
        (
            "/ab|ac/",
            Some("0000101000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/[ab]|c/",
            Some("0101101100000000111000000011000000011001000011000000000010010000000001101000"),
        ),
        (
            r"/.|\n/",
            Some("0111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/a|./",
            Some("0111111111111110111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/(?s:.)|a/",
            Some("0111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        ("/(/", None),
        ("/)/", None),
        ("/(a))/", None),
        ("/((a)/", None),
        ("/a)/", None),
        ("/[]/", None),
        ("/[^]/", None),
        ("/[a/", None),
        ("/[a-/", None),
        ("/[z-a]/", None),
        (r"/[\b]/", None),
        (r"/[\A]/", None),
        (r"/[\z]/", None),
        (r"/[a-\d]/", None),
        (
            r"/[\d-]/",
            Some("0000000000000000000000000000000001001011000100000000000000000000000000000000"),
        ),
        (
            "/[-a]/",
            Some("0100101100000000111000000011000000011011000011000000000010010000000001101000"),
        ),
        (
            r"/[a\-z]/",
            Some("0100101100000000111000000011000000011011000011000000000010010000000001101000"),
        ),
        (
            "/[[:alpha:]-z]/",
            Some("0111111111101100111000000011000000011011000111000000000011010000000001101000"),
        ),
        ("/[[:foo:]]/", None),
        ("/[[:alpha:]/", None),
        ("/[[:alpha]]/", None),
        ("/[[=a=]]/", None),
        ("/[[.a.]]/", None),
        (r"/[\Qa\E]/", None),
        (
            r"/[\p{Greek}\d]/",
            Some("0000000000000000000000000000111001001000000100000011000000000000011000000000"),
        ),
        (
            r"/[^\p{Greek}\d]/",
            Some("0111111111111111111111111111000110111111111111111100111111111111100111111111"),
        ),
        (
            r"/[\P{L}a]/i",
            Some("0110111100000001111111111011000011111111111111110000001111010011000111111111"),
        ),
        (
            r"/[\D\S]/",
            Some("0111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/[[:^word:][:digit:]]/",
            Some("0000000000010011111111111111111111101111110111111111111110111111111111011111"),
        ),
        (
            "/[\u{e9}-\u{fc}]/i",
            Some("0000000000000000000000000111000000000000000000000000000000000000000000000000"),
        ),
        (
            "/[k]/i",
            Some("0000000001110000000000000000000000000000000000000000000000110000000000000000"),
        ),
        (
            "/[K]/",
            Some("0000000001000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/[Aa]/",
            Some("0110111100000000111000000011000000011001000011000000000011010000000001101000"),
        ),
        (
            "/[Aa]b/i",
            Some("0000111000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/x[Kk]/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/[\u{17f}]/i",
            Some("0000000000001110000000000000000000000000000010000000000011000000000000000000"),
        ),
        (
            r"/[^\n]/",
            Some("0111111111111110111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/[^a]/s",
            Some("0011111011111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            r"/[\x{0}-\x{10FFFF}]/",
            Some("0111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            r"/[\x00-\x{10FFFF}]/i",
            Some("0111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/./",
            Some("0111111111111110111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/./s",
            Some("0111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/^.$/",
            Some("0111000011111110000111011100011010100110111000111111111100001111111110010111"),
        ),
        (
            "/(?s)^.$/",
            Some("0111000011111111000111011100011010100110111000111111111100001111111110010111"),
        ),
        (
            "/\u{df}/i",
            Some("0000000000000000000000000000000000000000000000001100000010000000000000000000"),
        ),
        (
            "/\u{3c3}/i",
            Some("0000000000000000000000000000011000000000000000000000000000000000000000000000"),
        ),
        (
            "/\u{b5}/i",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/\u{1c5}/i",
            Some("0000000000000000000000000000000000000000000000000000110000000000000000000000"),
        ),
        (
            "/i/i",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/\u{3c9}/i",
            Some("0000000000000000000000000000000000000000000000000011000000000000000000000000"),
        ),
        (
            "/\u{fb00}/i",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/(?i)ab|AB/",
            Some("0000111000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/\{/",
            Some("0000000000000000000000000000000000001100000000000000000000000000000000000000"),
        ),
        ("/a{1}{2}/", None),
        (
            "/(?i)a*/",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/a*?b/",
            Some("0001101000000000110000000000000000010001000001000000000000010000000001100000"),
        ),
        (
            "/(?U)a*?/",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/a/ims",
            Some("0110111100000000111000000011000000011001000011000000000011010000000001101000"),
        ),
        (
            "/a/mi",
            Some("0110111100000000111000000011000000011001000011000000000011010000000001101000"),
        ),
        (
            "/a/is",
            Some("0110111100000000111000000011000000011001000011000000000011010000000001101000"),
        ),
        ("/a/mm", None),
        ("/a/ss", None),
        ("/a/x", None),
        (
            "|a|i",
            Some("0110111100000000111000000011000000011001000011000000000011010000000001101000"),
        ),
        (
            "|a/b|",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/a|b/",
            Some("0101101100000000111000000011000000011001000011000000000010010000000001101000"),
        ),
        (
            "|a/b|i",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/a\/b/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/\//",
            Some("0000000000000000000000000000000000000000000110000000000000000000000000000000"),
        ),
        (
            r"/[\/]/",
            Some("0000000000000000000000000000000000000000000110000000000000000000000000000000"),
        ),
        (
            r"/\pN+/",
            Some("0000000000000000000000000000000001101000000100000000000000000000000000000001"),
        ),
        (
            r"/\p{Nd}/",
            Some("0000000000000000000000000000000001101000000100000000000000000000000000000000"),
        ),
        (
            r"/\d/",
            Some("0000000000000000000000000000000001001000000100000000000000000000000000000000"),
        ),
        (
            r"/\D/",
            Some("0111111111111111111111111111111110111111111111111111111111111111111111111111"),
        ),
        (
            r"/[\d]/",
            Some("0000000000000000000000000000000001001000000100000000000000000000000000000000"),
        ),
        (
            r"/[^\d]/",
            Some("0111111111111111111111111111111110111111111111111111111111111111111111111111"),
        ),
        (
            r"/\w+/",
            Some("0111111111101100111000000011000001011001001111000000000011010000000001101000"),
        ),
        (
            "/[[:alpha:]]+/",
            Some("0111111111101100111000000011000000011001000111000000000011010000000001101000"),
        ),
        (
            "/[[:^alpha:]]/",
            Some("0000000000010011111111111111111111111111111111111111111110111111111111111111"),
        ),
        (
            "/[[:punct:]]/",
            Some("0000000000000000000000000000000000011111111111000000000000000000000000100000"),
        ),
        (
            "/[[:graph:]]/",
            Some("0111111111101100111000000011000001011111111111000000000011010000000001101000"),
        ),
        (
            "/[[:print:]]/",
            Some("0111111111101100111001000011000001011111111111000000000011010000000001101000"),
        ),
        (
            "/[[:cntrl:]]/",
            Some("0000000000000001111110100000000000000000000000100000000000000000000110000000"),
        ),
        (
            "/[[:xdigit:]]/",
            Some("0111111100000000111000000011000001011001000111000000000011010000000001101000"),
        ),
        (
            "/[[:ascii:]]/",
            Some("0111111111101101111111100011000001011111111111100000000011010000000111101000"),
        ),
        (
            "/[[:^ascii:]]/",
            Some("0000000000010010000000011111111110100000000000011111111110111111111000011111"),
        ),
        (
            "/[[:alnum:]]/i",
            Some("0111111111111110111000000011000001011001000111000000000011110000000001101000"),
        ),
        (
            "/[[:digit:]]/i",
            Some("0000000000000000000000000000000001001000000100000000000000000000000000000000"),
        ),
        (
            "/[[:^lower:]]/i",
            Some("0000000000000001111111111111111111111111111111111111111110001111111111111111"),
        ),
        (
            "/[^[:lower:]]/",
            Some("0010010001010111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            r"/\x{1E943}/i",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/\x{1E921}/i",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/(?i)[\x{1E900}-\x{1E921}]/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/[^\x{1E900}-\x{1E921}]/i",
            Some("0111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            r"/(?i)\x{10400}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/(a)(b)(c)(d)/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/((((a))))/",
            Some("0100101100000000111000000011000000011001000011000000000010010000000001101000"),
        ),
        (
            "/(?:(?:(?:a)))/",
            Some("0100101100000000111000000011000000011001000011000000000010010000000001101000"),
        ),
        (
            "/(?i)(?-i:a)/",
            Some("0100101100000000111000000011000000011001000011000000000010010000000001101000"),
        ),
        (
            "/(?i:a|b)c/",
            Some("0000001000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/a(?i)b|c/",
            Some("0000101000000000000000000000000000000000000100000000000000000000000000000000"),
        ),
        (
            "/(a(?i)b)c/",
            Some("0000001000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/(?m:$)/",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/(?m)$^/",
            Some("1000000000000001001000100000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/$a/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/^$/",
            Some("1000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/(?s:a.b)/",
            Some("0000000000000000110000000000000000000001000001000000000000010000000001100000"),
        ),
        (
            r"/\p{Ll}/",
            Some("0101101110101010111000000111101000011001000011001001010010010100011001101000"),
        ),
        (
            r"/\x{A7CE}/i",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/\x{295}/i",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/[\x{A7CE}]/i",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/\p{Lo}/",
            Some("0000000000000000000000000000000100000000000000000000000000000000000000000000"),
        ),
        (
            r"/\p{Mn}/i",
            Some("0000000000000000000000000000000000000000000000000000000000000000111000011000"),
        ),
        (
            r"/\P{Ll}/i",
            Some("0000000000000001111111111000000111111111111111110000001100001011000111111111"),
        ),
        (
            r"/[^\P{Ll}]/i",
            Some("0111111111111110111000000111111000011001000111001111110011110100111001101000"),
        ),
        (
            r"/\p{Latin}/",
            Some("0111111111111110111000000111000000011001000111001100110011111100000001101000"),
        ),
        (
            r"/\x{1E9E}/i",
            Some("0000000000000000000000000000000000000000000000001100000010000000000000000000"),
        ),
        (
            r"/[\x{100}-\x{17F}]/i",
            Some("0000000000001110000000000000000000000000000010000000000011001100000000000000"),
        ),
        (
            r"/[^\x{100}-\x{17F}]/i",
            Some("0111111111110001111111111111111111111111111111111111111111110011111111111111"),
        ),
        (
            "/(?i)[^k]/",
            Some("0111111110001111111111111111111111111111111111111111111111011111111111111111"),
        ),
        (
            r"/(?i)[^\x{212A}]/",
            Some("0111111110001111111111111111111111111111111111111111111111011111111111111111"),
        ),
        (
            "/[[:^space:]]/",
            Some("0111111111111110111000011111111111111111111111111111111111111111111001111111"),
        ),
        (
            r"/\p{Zs}/",
            Some("0000000000000000000001010000000000000000000000000000000000000000000001000100"),
        ),
        (
            r"/\p{Z}/",
            Some("0000000000000000000001011000000000000000000000000000000000000000000001000100"),
        ),
        (
            r"/\p{Zl}/",
            Some("0000000000000000000000001000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/\p{Cc}/",
            Some("0000000000000001111110100000000000000000000000100000000000000000000110000010"),
        ),
        (
            r"/\p{Cf}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/\p{Co}/",
            Some("0000000000000000000000000000000000000000000000000000000100000000000000000000"),
        ),
        (
            r"/\p{L}+\p{N}*/",
            Some("0111111111111110111000000111111100011001000111001111110011111100011001101000"),
        ),
        (
            "/(?i)stra\u{df}e/",
            Some("0000000000000000000000000000000000000000000000000000000010000000000000000000"),
        ),
        (
            "/(?i)STRASSE/",
            Some("0000000000000000000000000000000000000000000000000000000001000000000000000000"),
        ),
        (
            "/16 or host matches /",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/^a/",
            Some("0100101100000000101000000010000000001001000001000000000000010000000001101000"),
        ),
        (
            "/pattern/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/[\W]/",
            Some("0000000000010011111111111111111110101111110111111111111110111111111111011111"),
        ),
        (
            r"/[^\S]/",
            Some("0000000000000001111011100000000000000000000000000000000000000000000111000000"),
        ),
        (
            r"/[\D]/",
            Some("0111111111111111111111111111111110111111111111111111111111111111111111111111"),
        ),
        (
            r"/a\Bb/",
            Some("0000101000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/\B\x{212A}/",
            Some("0000000000010000000000000000000000000000000000000000000000100000000000000000"),
        ),
        (
            r"/\p{Old Italic}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/\p{any}/",
            Some("0111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            r"/\P{ASCII}/i",
            Some("0000000000000000000000011111111110100000000000011111111110001111111000011111"),
        ),
        (
            "/[]-]/",
            Some("0000000000000000000000000000000000000011100000000000000000000000000000000000"),
        ),
        (
            "/(?i)k/",
            Some("0000000001110000000000000000000000000000000000000000000000110000000000000000"),
        ),
        (
            "/(?s:.)/",
            Some("0111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/^$/m",
            Some("1000000000000001001000100000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/(a{2}){500}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/(?:(?:a{3}){0}){999}/",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/(?:a{2,}){500}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/(?:a{30}|b{40}){25}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/[\x00-\x{10FFFF}]/",
            Some("0111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/(?m)^x*/ms",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (r"/\08/", None),
        ("/(?P<\u{b2}>a)/", None),
        (
            r"/[\ ]/",
            Some("0000000000000000000001000000000000000000000000000000000000000000000001000000"),
        ),
        (r"/[^\P{Alnum}]/i", None),
        (r"/[\B]/", None),
        (r"/[\p{IsGreek}]/", None),
        (r"/\p2/", None),
        (
            "/[\\\u{c}]/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000100000000"),
        ),
        (
            r"/\P{Phags_Pa}/i",
            Some("0111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/[^[:^lower:]]/",
            Some("0101101110101000111000000011000000011001000011000000000010010000000001101000"),
        ),
        ("/(?<!a)a+b/", None),
        (r"/a\5b/i", None),
        ("/[^\\P{Gre\u{e9}k}]/i", None),
        (
            r"/\P{Z}/",
            Some("0111111111111111111110100111111111111111111111111111111111111111111111111011"),
        ),
        (
            "/a\\\u{1c}b/i",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        ("/[x[::]]/i", None),
        (r"/[\<]/", None),
        (r"/[^\P{RI}]/", None),
        (
            r"/\p{Unassigned}/",
            Some("0000000000000000000000000000000000000000000000010000001000000000000000000000"),
        ),
        (
            "/\\\u{e}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (r"/\pr/", None),
        (r"/\u/", None),
        (
            "/[x[:^space:]]/i",
            Some("0111111111111110111000011111111111111111111111111111111111111111111001111111"),
        ),
        (
            "/a\\\u{11}b/i",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (r"/\pQ/", None),
        (r"/\P7/i", None),
        (
            "/\\\u{1}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/[^[:space:]]/",
            Some("0111111111111110111000011111111111111111111111111111111111111111111001111111"),
        ),
        ("/(?<a b>a)a+b/", None),
        (r"/[\p{bc=L}]/i", None),
        (
            r"/\x{00000000000000041}/i",
            Some("0110111100000000111000000011000000011001000011000000000011010000000001101000"),
        ),
        ("/a|*/i", None),
        (
            r"/[^\P{Z}]/",
            Some("0000000000000000000001011000000000000000000000000000000000000000000001000100"),
        ),
        (r"/\o{101}/i", None),
        (r"/\P{Blank}/i", None),
        (r"/[\j]/", None),
        (
            r"/\PC/i",
            Some("0111111111111110111001011111111111111111111111001111110011111111111001111101"),
        ),
        (
            r"/\n$/ms",
            Some("0000000000000001001000100000000000000000000000000000000000000000000000000000"),
        ),
        (r"/\Pf/i", None),
        (
            r"/[\a-\f]/",
            Some("0000000000000001111110100000000000000000000000000000000000000000000100000000"),
        ),
        (r"/a\Pb/i", None),
        (
            r"/\PZ/i",
            Some("0111111111111111111110100111111111111111111111111111111111111111111111111011"),
        ),
        (r"/\pD/", None),
        (r"/\P{Script=Greek}/", None),
        (r"/\H/", None),
        (r"/\p{Extended_Pictographic}/", None),
        (
            "/\\\u{13}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (r"/\Po/i", None),
        (
            "/a\\\u{e}b/i",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/[[:cntrl:]]/i",
            Some("0000000000000001111110100000000000000000000000100000000000000000000110000000"),
        ),
        (r"/[^\P{Unknown}]/", None),
        (r"/[^\P{Space}]/", None),
        (r"/[^\P{}]/", None),
        (
            "/(?s:.)/ms",
            Some("0111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            r"/[\p{Uppercase Letter}]/i",
            Some("0111111111111110111000000111111000011001000111001111110011111000111001101000"),
        ),
        (
            r"/\p{Digit}/i",
            Some("0000000000000000000000000000000001101000000100000000000000000000000000000000"),
        ),
        (
            r"/\P{Old-Italic}/i",
            Some("0111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/(){1000}/i",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            r"/\w*/s",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            r"/[^\x{0}-\x{10FFFF}]a/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/\s/m",
            Some("0000000000000001111011100000000000000000000000000000000000000000000111000000"),
        ),
        (
            r"|\/|",
            Some("0000000000000000000000000000000000000000000110000000000000000000000000000000"),
        ),
        (r"/\PJ/i", None),
        (
            "/(?:^){2}/i",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (r"/[\p{Uppercase}]/", None),
        (r"/\p{Unknown}/", None),
        (
            r"/\p{Other_Letter}/i",
            Some("0000000000000000000000000000000100000000000000000000000000000000000000000000"),
        ),
        (
            r"/[\p{G_r_e_e_k}]/i",
            Some("0000000000000000000000000000111000000000000000000011000000000000111000000000"),
        ),
        (
            "/(|)*/",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            r"/a\{2\}/i",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/[\!]/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (r"/a\jb/i", None),
        (
            r"/[\p{Ol Chiki}]/i",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        ("/[[:alpha:]/i", None),
        (
            r"/\p{Lt}/",
            Some("0000000000000000000000000000000000000000000000000000100000000000000000000000"),
        ),
        (r"/\x{7FFFFFFF}/i", None),
        (r"/[^\P{Qaai}]/i", None),
        (
            r"/[\p{Cn}]/i",
            Some("0000000000000000000000000000000000000000000000010000001000000000000000000000"),
        ),
        (
            "/(?<n>a)a+b/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/a\(b/i",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (r"/\pb/", None),
        (r"/\pH/", None),
        (
            "/(?s:.)/s",
            Some("0111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            r"/\x{345}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000100000000000"),
        ),
        (
            r"/\p{So}/i",
            Some("0000000000000000000000000000000010000000000000000000000000000011000000000000"),
        ),
        ("/)/i", None),
        (r"/[^\P{Unknown}]/i", None),
        (r"/\pW/", None),
        (
            r"/[\p{punct}]/i",
            Some("0000000000000000000000000000000000011111111111000000000000000000000000100000"),
        ),
        (
            "/\\\u{1b}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/a\@b/i",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/(?:a*){1000}/i",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            r"/[^\P{Symbol}]/i",
            Some("0000000000000000000000000000000010000000000000000000000000000011000000000000"),
        ),
        (
            "/\\\u{16}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/\\\u{10}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/a$\n^b/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/\pS/",
            Some("0000000000000000000000000000000010000000000000000000000000000011000000000000"),
        ),
        (
            r"/\p{-Greek-}/",
            Some("0000000000000000000000000000111000000000000000000011000000000000011000000000"),
        ),
        ("/a{/i", None),
        (
            r"/[^\p{ASCII}]/i",
            Some("0000000000000000000000011111111110100000000000011111111110001111111000011111"),
        ),
        ("/(?x)a+b/", None),
        (
            r"/[\p{decimalnumber}]/",
            Some("0000000000000000000000000000000001101000000100000000000000000000000000000000"),
        ),
        (
            r"/[^\P{GREEK}]/i",
            Some("0000000000000000000000000000111000000000000000000011000000000000111000000000"),
        ),
        (
            r"/[\pL]/",
            Some("0111111111111110111000000111111100011001000111001111110011111100011001101000"),
        ),
        (r"/[\i]/", None),
        (r"/\P{Hrkt}/i", None),
        (
            "/a\\\rb/i",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/[x[:^print:]]/i",
            Some("0000000010000001111110111111111110100000000000111111111110001111111110011111"),
        ),
        ("/a{2, 3}/i", None),
        (
            "/\\\u{7f}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/\w*/",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (r"/[\p{Alphabetic}]/", None),
        (
            r"/[\-]/",
            Some("0000000000000000000000000000000000000011000000000000000000000000000000000000"),
        ),
        (
            r"/[\d\D]/i",
            Some("0111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            r"/[^\P{G_r_e_e_k}]/",
            Some("0000000000000000000000000000111000000000000000000011000000000000011000000000"),
        ),
        (r"/[\p{Emoji}]/i", None),
        (r"/[^\P{Extended_Pictographic}]/i", None),
        ("/a*+/i", None),
        (r"/[\xG]/", None),
        ("/\\P{Gr\u{e9}ek}/", None),
        (r"/\P{RI}/", None),
        ("/a?*/", None),
        (
            r"/[^\P{l}]/",
            Some("0111111111111110111000000111111100011001000111001111110011111100011001101000"),
        ),
        (
            r"/[^\P{old italic}]/i",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/[x[:^xdigit:]]/i",
            Some("0000000011111111111111111111111110111111111111111111111111111111111111111111"),
        ),
        (r"/[\08]/", None),
        (r"/[\1]/", None),
        (
            "/[.]/i",
            Some("0000000000000000000000000000000000000000000101000000000000000000000000000000"),
        ),
        (r"/\08/i", None),
        ("/[^[:Alpha:]]/i", None),
        ("/[[:^Alpha:]]/i", None),
        (
            "/K/ims",
            Some("0000000001110000000000000000000000000000000000000000000000110000000000000000"),
        ),
        (r"/[\p{Zinh}]/i", None),
        (r"/\p{/", None),
        (
            "/a]/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/^b/i",
            Some("0001000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (r"/\g1/i", None),
        (r"/\P{Alnum}/", None),
        (r"/\P{Grek}/i", None),
        (r"/(a)\1/", None),
        (
            r"/\Ps/i",
            Some("0111111111111111111111111111111101111111111111111111111111111100111111111111"),
        ),
        (
            r"/\A\z/s",
            Some("1000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (r"/a\xb/i", None),
        (
            r"/[\p{Greek}}]/",
            Some("0000000000000000000000000000111000001000000000000011000000000000011000000000"),
        ),
        (
            "/\\\u{7}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/[^\P{punct}]/i",
            Some("0000000000000000000000000000000000011111111111000000000000000000000000100000"),
        ),
        (r"/\Pd/i", None),
        (r"/[\p{Alphabetic}]/i", None),
        (r"/\p{White_Space}/i", None),
        (r"/[\Q]/", None),
        (r"/[\p{Zyyy}]/i", None),
        (r"/[^\P{White_Space}]/i", None),
        (
            "/\\\u{12}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/[\W\d]/i",
            Some("0000000000000001111111111111111111101111110111111111111110001111111111011111"),
        ),
        (
            "/\\\u{19}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/[^[:digit:]]/i",
            Some("0111111111111111111111111111111110111111111111111111111111111111111111111111"),
        ),
        ("/\\P{Greek\n}/i", None),
        (
            "/[:alpha:]/i",
            Some("0110111100000000111000000011000000011001000111000000000011010000000001101000"),
        ),
        (
            r"/\\/",
            Some("0000000000000000000000000000000000000000010000000000000000000000000000000000"),
        ),
        (
            "/[^K]/i",
            Some("0111111110001111111111111111111111111111111111111111111111011111111111111111"),
        ),
        (
            "/\t/i",
            Some("0000000000000000000010000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/\\\u{5}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (r"/[\O]/", None),
        (
            "/a/",
            Some("0100101100000000111000000011000000011001000011000000000010010000000001101000"),
        ),
        (r"/\Pt/i", None),
        (r"/\P{Print}/", None),
        (r"/a\Mb/i", None),
        (
            r"/\P{ Greek }/i",
            Some("0111111111111111111111111111000111111111111111111100111111111111000111111111"),
        ),
        (
            "/[\\\u{1d}]/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/[x[:^cntrl:]]/i",
            Some("0111111111111110111001011111111111111111111111011111111111111111111001111111"),
        ),
        (
            r"/\P{Letter}/i",
            Some("0000000000000001111111111000000011111111111111110000001100000011000111111111"),
        ),
        (r"/\K/i", None),
        (
            r"/\b?/s",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            r"/\p{Any}/i",
            Some("0111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/\\\t/i",
            Some("0000000000000000000010000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/[\p{ Greek }]/i",
            Some("0000000000000000000000000000111000000000000000000011000000000000111000000000"),
        ),
        (
            r"/\p{Han}/i",
            Some("0000000000000000000000000000000100000000000000000000000000000000000000000000"),
        ),
        (
            r"/a\rb/i",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/[\f]/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000100000000"),
        ),
        (r"/[\p{Qaai}]/", None),
        (
            r"/[^\P{Uppercase Letter}]/",
            Some("0010010001010100000000000000010000000000000100000110000001111000000000000000"),
        ),
        (
            r"/[^\P{Latin}]/i",
            Some("0111111111111110111000000111000000011001000111001100110011111100000001101000"),
        ),
        (r"/\p{Zinh}/", None),
        (
            "/(?m)$^/s",
            Some("1000000000000001001000100000000000000000000000000000000000000000000000000000"),
        ),
        (r"/[\5]/", None),
        (r"/\O/", None),
        (
            r"/\p{Lm}/i",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        ("/(?:a{0}){1001}/", None),
        (r"/[\p{Latn}]/", None),
        (
            "/[[:^alpha:]]/i",
            Some("0000000000000001111111111111111111111111111111111111111110001111111111111111"),
        ),
        (
            r"/[^\P{Greek}}]/i",
            Some("0000000000000000000000000000111000000000000000000011000000000000111000000000"),
        ),
        ("/(?u)a+b/", None),
        (
            "/[x[:^blank:]]/i",
            Some("0111111111111111111100111111111111111111111111111111111111111111111111111111"),
        ),
        (r"/\p{w}/", None),
        (r"/\N{LATIN SMALL LETTER A}/", None),
        (r"/[\p{XDigit}]/i", None),
        ("/(?i-)a+b/", None),
        (
            "/.{3}/",
            Some("0000001000000000000000000000100001011001000111000000000011010000000001100000"),
        ),
        (r"/a\<b/i", None),
        ("/a{1001}/i", None),
        (r"/\p{Hex_Digit}/", None),
        (r"/\p{isL}/i", None),
        (
            r"/[^\s]/",
            Some("0111111111111110111100011111111111111111111111111111111111111111111001111111"),
        ),
        (
            r"/\p{Old_Italic}/i",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/[[:^space:]]/i",
            Some("0111111111111110111000011111111111111111111111111111111111111111111001111111"),
        ),
        (r"/\P{XPosixAlpha}/", None),
        (r"/[^\P{Blank}]/i", None),
        (
            r"/\p{Nko}/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/\u{1c5}/",
            Some("0000000000000000000000000000000000000000000000000000100000000000000000000000"),
        ),
        (
            "/[^[:^space:]]/i",
            Some("0000000000000001111111100000000000000000000000000000000000000000000111000000"),
        ),
        (
            r"/[\x{2120}-\x{2130}]/",
            Some("0000000000010000000000000000000000000000000000000010000000110000000000000000"),
        ),
        ("/[[::]]/", None),
        (r"/\p{Emoji}/i", None),
        (
            "/[x[:word:]]/i",
            Some("0111111111111110111000000011000001011001001111000000000011110000000001101000"),
        ),
        (
            "/[^[:^alpha:]]/i",
            Some("0111111111111110111000000011000000011001000111000000000011110000000001101000"),
        ),
        (r"/[\p{Gre\k}]/i", None),
        (r"/[^\P{Katakana_Or_Hiragana}]/i", None),
        ("/(?a)a+b/", None),
        (
            "/(?iU)a+b/",
            Some("0000111000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        ("/a{2}*/i", None),
        (
            r"/\p{Symbol}/",
            Some("0000000000000000000000000000000010000000000000000000000000000011000000000000"),
        ),
        (
            "/a{2,3}/i",
            Some("0000000100000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/[J-L]/",
            Some("0000000001000000000000000000000000000000000100000000000000000000000000000000"),
        ),
        (
            r"/a\.b/",
            Some("0000000000000000000000000000000000000000000001000000000000000000000000000000"),
        ),
        (
            r"/[\p{Lu}\p{Ll}]/",
            Some("0111111111111110111000000111111000011001000111001111010011111100011001101000"),
        ),
        (
            r"/[^\P{-Greek-}]/",
            Some("0000000000000000000000000000111000000000000000000011000000000000011000000000"),
        ),
        (
            r"/\p{Zs}/i",
            Some("0000000000000000000001010000000000000000000000000000000000000000000001000100"),
        ),
        (
            r"/[^\P{ Greek }]/i",
            Some("0000000000000000000000000000111000000000000000000011000000000000111000000000"),
        ),
        (
            r"/a\,b/i",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/[^[:^punct:]]/i",
            Some("0000000000000000000000000000000000011111111111000000000000000000000000100000"),
        ),
        (
            "/[[:^word:]]/",
            Some("0000000000010011111111111111111110101111110111111111111110111111111111011111"),
        ),
        (
            r"/[^\P{Mark}]/i",
            Some("0000000000000000000000000000000000000000000000000000000000000000111000011000"),
        ),
        (
            "/[x[:alnum:]]/i",
            Some("0111111111111110111000000011000001011001000111000000000011110000000001101000"),
        ),
        (
            r"/\x{131}/i",
            Some("0000000000000000000000000000000000000000000000000000000000000100000000000000"),
        ),
        (
            "/[x[:^blank:]]/",
            Some("0111111111111111111100111111111111111111111111111111111111111111111111111111"),
        ),
        (
            r"/\P{Common}/i",
            Some("0111111111111110111000000111111100111001000111011111111111111100111001111000"),
        ),
        (r"/\0/i", None),
        (
            r"/\W/",
            Some("0000000000010011111111111111111110101111110111111111111110111111111111011111"),
        ),
        (r"/[^\P{Latn}]/i", None),
        (
            r"/\P{L}/i",
            Some("0000000000000001111111111000000011111111111111110000001100000011000111111111"),
        ),
        (r"/\4/", None),
        (r"/a\mb/i", None),
        (
            "/[^[:^alpha:]]/",
            Some("0111111111101100111000000011000000011001000111000000000011010000000001101000"),
        ),
        (r"/\P{Latn}/", None),
        (
            r"/[\t-\n]/",
            Some("0000000000000001111010100000000000000000000000000000000000000000000000000000"),
        ),
        ("/\\p{Greek\u{a0}}/", None),
        (
            r"/\x{A7D2}/i",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (r"/a\6b/i", None),
        (
            r"/\P{Lowercase_Letter}/",
            Some("0010010001010101111111111000010111111111111111110110101101111011100111111111"),
        ),
        (r"/[\p{Emoji}]/", None),
        (
            "/[^]a]/i",
            Some("0001111011111111111111111111111111111111011111111111111111111111111111111111"),
        ),
        (r"/[\b]/i", None),
        (
            r"/a\{b/i",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        ("/(?-u)a+b/", None),
        (
            r"/[\p{Uppercase Letter}]/",
            Some("0010010001010100000000000000010000000000000100000110000001111000000000000000"),
        ),
        (r"/[\cA]/", None),
        (
            "/}/",
            Some("0000000000000000000000000000000000001000000000000000000000000000000000000000"),
        ),
        (
            r"/[\x{24B6}]/",
            Some("0000000000000000000000000000000000000000000000000000000000000010000000000000"),
        ),
        (
            r"/\x{a}/",
            Some("0000000000000001111000100000000000000000000000000000000000000000000000000000"),
        ),
        (r"/[\p{Extended_Pictographic}]/", None),
        (r"/\P{Extended_Pictographic}/", None),
        (
            r"/\x{41}/i",
            Some("0110111100000000111000000011000000011001000011000000000011010000000001101000"),
        ),
        (
            "/(?m)/",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/a.b/is",
            Some("0000000000000000110000000000000000000001000001000000000000010000000001100000"),
        ),
        (r"/a\Ob/i", None),
        (r"/\p{Word}/i", None),
        (r"/\x{41 }/i", None),
        (
            r"/[\x{17E}-\x{180}]/",
            Some("0000000000000010000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/[x[:graph:]]/",
            Some("0111111111101100111000000011000001011111111111000000000011010000000001101000"),
        ),
        (
            r"/\b?/i",
            Some("1111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/[[:^graph:]]/i",
            Some("0000000000000001111111111111111110100000000000111111111110001111111111011111"),
        ),
        ("/(?:a{1000}){2}/", None),
        (
            r"/\p{Sign_Writing}/i",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (r"/\P9/i", None),
        (
            "/[A-Z]/i",
            Some("0111111111111110111000000011000000011001000111000000000011110000000001101000"),
        ),
        (
            "/a{2}?/i",
            Some("0000000100000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (r"/\P{Age=1.1}/", None),
        (
            "/[\\\u{10}]/",
            Some("0000000000000000000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r"/\p{GREEK}/i",
            Some("0000000000000000000000000000111000000000000000000011000000000000111000000000"),
        ),
        (
            r"/[\p{cntrl}]/i",
            Some("0000000000000001111110100000000000000000000000100000000000000000000110000010"),
        ),
        (r"/\p3/", None),
        (r"/\p{Gre\k}/i", None),
        (
            r"/[^\P{C}]/",
            Some("0000000000000001111110100000000000000000000000110000001100000000000110000010"),
        ),
        (
            r"/\P{Sign_Writing}/i",
            Some("0111111111111111111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            r"/[\x{41}]/",
            Some("0010010000000000000000000000000000000000000000000000000001000000000000000000"),
        ),
    ];
}
