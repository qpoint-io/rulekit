//! Regex literals.
//!
//! A rulekit regex literal (`/pattern/flags` or `|pattern|flags`) means
//! exactly what Go rulekit's `parseRegex` makes of it. This module ports
//! `parseRegex` and `checkRegexDialect`, then ports Go's `regexp/syntax`
//! parser (Perl flags, as used by `regexp.Compile`): it accepts and rejects
//! the same patterns, with the same error messages, and builds the same parse
//! tree. The tree is then printed as a pattern for the `regex` crate in which
//! every character class (`\pL`, `[[:alpha:]]`, `\w`, `.`, case-folded
//! literals under `(?i)`, ...) is spelled out as explicit code point ranges
//! computed from Go's Unicode tables and Go's case folding, so matching does
//! not depend on the `regex` crate's Unicode version or case folding rules.

mod unicode_names;

use std::fmt;
use std::fmt::Write as _;

use unicode_names::{GO_UNICODE_NAMES, SIMPLE_FOLD};

/// Go's budget for a compiled regexp: `regexp/syntax` sizes `maxSize` and
/// `maxRunes` from 128 MB. Used as the `regex` crate's compiled size limit, so
/// patterns Go accepts (such as `\pL{1000}`) are not rejected by the default
/// 10 MB limit.
const GO_PROGRAM_BUDGET: usize = 128 << 20;

/// Nesting limit for the translated pattern. Go limits its parse tree to
/// `MAX_HEIGHT` levels, and the translation adds at most a few `regex`
/// nesting levels per Go tree level.
const NEST_LIMIT: u32 = 8 * MAX_HEIGHT as u32;

/// Compiles a regex literal token as lexed: `/pattern/flags` or
/// `|pattern|flags`, where flags are zero or more of `i`, `m`, `s`.
pub(crate) fn compile_literal(raw: &str) -> Result<regex::Regex, String> {
    let pattern = literal_pattern(raw)?;
    check_regex_dialect(&pattern)?;
    let tree = parse(&pattern).map_err(|err| err.to_string())?;
    let mut translated = String::new();
    emit(&tree, &mut translated);
    regex::RegexBuilder::new(&translated)
        .size_limit(GO_PROGRAM_BUDGET)
        .nest_limit(NEST_LIMIT)
        .build()
        .map_err(|err| match err {
            regex::Error::CompiledTooBig(_) => {
                SyntaxError::new(ErrorCode::Large, &pattern).to_string()
            }
            err => err.to_string(),
        })
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

// ---------------------------------------------------------------------------
// Port of Go regexp/syntax (parse.go, perl_groups.go), Perl flags.

/// Go `syntax.ErrorCode` (the codes the Perl-mode parser can produce for
/// valid UTF-8 input).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ErrorCode {
    InvalidCharRange,
    InvalidEscape,
    InvalidNamedCapture,
    InvalidPerlOp,
    InvalidRepeatOp,
    InvalidRepeatSize,
    MissingBracket,
    MissingParen,
    MissingRepeatArgument,
    TrailingBackslash,
    UnexpectedParen,
    NestingDepth,
    Large,
}

impl ErrorCode {
    fn as_str(self) -> &'static str {
        match self {
            ErrorCode::InvalidCharRange => "invalid character class range",
            ErrorCode::InvalidEscape => "invalid escape sequence",
            ErrorCode::InvalidNamedCapture => "invalid named capture",
            ErrorCode::InvalidPerlOp => "invalid or unsupported Perl syntax",
            ErrorCode::InvalidRepeatOp => "invalid nested repetition operator",
            ErrorCode::InvalidRepeatSize => "invalid repeat count",
            ErrorCode::MissingBracket => "missing closing ]",
            ErrorCode::MissingParen => "missing closing )",
            ErrorCode::MissingRepeatArgument => "missing argument to repetition operator",
            ErrorCode::TrailingBackslash => "trailing backslash at end of expression",
            ErrorCode::UnexpectedParen => "unexpected )",
            ErrorCode::NestingDepth => "expression nests too deeply",
            ErrorCode::Large => "expression too large",
        }
    }
}

/// Go `*syntax.Error`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SyntaxError {
    code: ErrorCode,
    expr: String,
}

impl SyntaxError {
    fn new(code: ErrorCode, expr: &str) -> Self {
        SyntaxError {
            code,
            expr: expr.to_owned(),
        }
    }
}

impl fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "error parsing regexp: {}: `{}`",
            self.code.as_str(),
            self.expr
        )
    }
}

type ParseResult<T> = Result<T, SyntaxError>;

// Go syntax.Flags.
const FOLD_CASE: u16 = 1;
const CLASS_NL: u16 = 1 << 2;
const DOT_NL: u16 = 1 << 3;
const ONE_LINE: u16 = 1 << 4;
const NON_GREEDY: u16 = 1 << 5;
const PERL_X: u16 = 1 << 6;
const UNICODE_GROUPS: u16 = 1 << 7;
const PERL: u16 = CLASS_NL | ONE_LINE | PERL_X | UNICODE_GROUPS;

const MAX_HEIGHT: usize = 1000;
const MAX_SIZE: i64 = (128 << 20) / 40; // instSize = 5 * 8
const MAX_RUNES: usize = (128 << 20) / 4; // runeSize = 4
const MAX_RUNE: u32 = 0x10FFFF;
const MIN_FOLD: u32 = 0x0041;
const MAX_FOLD: u32 = 0x1e943;

/// Go `syntax.Op`, in Go's order (the parser compares ops).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Op {
    NoMatch,
    EmptyMatch,
    Literal,
    CharClass,
    AnyCharNotNL,
    AnyChar,
    BeginLine,
    EndLine,
    BeginText,
    EndText,
    WordBoundary,
    NoWordBoundary,
    Capture,
    Star,
    Plus,
    Quest,
    Repeat,
    Concat,
    Alternate,
    // Pseudo-ops for the parse stack (Go opPseudo and above).
    LeftParen,
    VerticalBar,
}

impl Op {
    fn is_pseudo(self) -> bool {
        self >= Op::LeftParen
    }
}

/// Go `syntax.Regexp`, plus the cached height and size Go's limit checks
/// compute.
#[derive(Debug)]
struct Node {
    op: Op,
    flags: u16,
    /// Literal: the runes. CharClass: flat lo, hi pairs.
    runes: Vec<u32>,
    min: i32,
    max: i32,
    /// LeftParen/Capture: capture index; 0 for a non-capturing group.
    cap: usize,
    subs: Vec<Node>,
    height: usize,
    size: i64,
}

impl Node {
    fn new(op: Op, flags: u16) -> Self {
        Node {
            op,
            flags,
            runes: Vec::new(),
            min: 0,
            max: 0,
            cap: 0,
            subs: Vec::new(),
            height: 1,
            size: 1,
        }
    }

    fn fold(&self) -> bool {
        self.flags & FOLD_CASE != 0
    }

    /// Recomputes height (Go calcHeight) and size (Go calcSize) from the
    /// direct subexpressions.
    fn update(&mut self) {
        self.height = 1 + self.subs.iter().map(|s| s.height).max().unwrap_or(0);
        let sub = self.subs.first().map_or(0, |s| s.size);
        let size = match self.op {
            Op::Literal => self.runes.len() as i64,
            Op::Capture | Op::Star => 2 + sub,
            Op::Plus | Op::Quest => 1 + sub,
            Op::Concat => self.subs.iter().map(|s| s.size).sum(),
            Op::Alternate => {
                let n = self.subs.len() as i64;
                self.subs.iter().map(|s| s.size).sum::<i64>() + if n > 1 { n - 1 } else { 0 }
            }
            Op::Repeat if self.max == -1 && self.min == 0 => 2 + sub,
            Op::Repeat if self.max == -1 => 1 + i64::from(self.min) * sub,
            Op::Repeat => i64::from(self.max) * sub + i64::from(self.max - self.min),
            _ => 0,
        };
        self.size = size.max(1);
    }
}

struct Parser<'a> {
    s: &'a str,
    b: &'a [u8],
    flags: u16,
    stack: Vec<Node>,
    num_cap: usize,
    num_runes: usize,
}

/// Port of Go `syntax.Parse(s, syntax.Perl)`.
fn parse(s: &str) -> ParseResult<Node> {
    let mut p = Parser {
        s,
        b: s.as_bytes(),
        flags: PERL,
        stack: Vec::new(),
        num_cap: 0,
        num_runes: 0,
    };
    let mut t = 0;
    let mut last_repeat: Option<usize> = None;
    while t < p.b.len() {
        let mut repeat = None;
        match p.b[t] {
            b'(' => {
                if p.flags & PERL_X != 0 && p.b.get(t + 1) == Some(&b'?') {
                    // Flag changes and non-capturing groups.
                    t = p.parse_perl_flags(t)?;
                } else {
                    p.num_cap += 1;
                    let mut re = Node::new(Op::LeftParen, p.flags);
                    re.cap = p.num_cap;
                    p.push(re)?;
                    t += 1;
                }
            }
            b'|' => {
                p.parse_vertical_bar()?;
                t += 1;
            }
            b')' => {
                p.parse_right_paren()?;
                t += 1;
            }
            b'^' => {
                p.op(if p.flags & ONE_LINE != 0 {
                    Op::BeginText
                } else {
                    Op::BeginLine
                })?;
                t += 1;
            }
            b'$' => {
                p.op(if p.flags & ONE_LINE != 0 {
                    Op::EndText
                } else {
                    Op::EndLine
                })?;
                t += 1;
            }
            b'.' => {
                p.op(if p.flags & DOT_NL != 0 {
                    Op::AnyChar
                } else {
                    Op::AnyCharNotNL
                })?;
                t += 1;
            }
            b'[' => t = p.parse_class(t)?,
            c @ (b'*' | b'+' | b'?') => {
                let op = match c {
                    b'*' => Op::Star,
                    b'+' => Op::Plus,
                    _ => Op::Quest,
                };
                let before = t;
                t = p.repeat(op, 0, 0, before, before + 1, last_repeat)?;
                repeat = Some(before);
            }
            b'{' => {
                let before = t;
                match p.parse_repeat(t) {
                    None => {
                        // If the repeat cannot be parsed, { is a literal.
                        p.literal(u32::from('{'))?;
                        t += 1;
                    }
                    Some((min, max, after)) => {
                        if !(0..=1000).contains(&min) || max > 1000 || max >= 0 && min > max {
                            // Numbers were too big, or max is present and min > max.
                            return Err(SyntaxError::new(
                                ErrorCode::InvalidRepeatSize,
                                &s[before..after],
                            ));
                        }
                        t = p.repeat(Op::Repeat, min, max, before, after, last_repeat)?;
                        repeat = Some(before);
                    }
                }
            }
            b'\\' => t = p.parse_backslash(t)?,
            _ => {
                let (c, next) = p.next_rune(t);
                p.literal(c)?;
                t = next;
            }
        }
        last_repeat = repeat;
    }

    p.concat()?;
    if p.swap_vertical_bar() {
        p.stack.pop(); // pop vertical bar
    }
    p.alternate()?;

    if p.stack.len() != 1 {
        return Err(SyntaxError::new(ErrorCode::MissingParen, s));
    }
    Ok(p.stack.pop().expect("stack has one element"))
}

impl Parser<'_> {
    fn next_rune(&self, t: usize) -> (u32, usize) {
        match self.s[t..].chars().next() {
            Some(c) => (u32::from(c), t + c.len_utf8()),
            // Go's nextRune("") yields utf8.RuneError without consuming input.
            None => (0xFFFD, t),
        }
    }

    fn error(&self, code: ErrorCode, from: usize, to: usize) -> SyntaxError {
        SyntaxError::new(code, &self.s[from..to])
    }

    fn check_limits(&self, re: &Node) -> ParseResult<()> {
        if self.num_runes > MAX_RUNES || re.size > MAX_SIZE {
            return Err(SyntaxError::new(ErrorCode::Large, self.s));
        }
        if re.height > MAX_HEIGHT {
            return Err(SyntaxError::new(ErrorCode::NestingDepth, self.s));
        }
        Ok(())
    }

    /// Go `push`.
    fn push(&mut self, mut re: Node) -> ParseResult<()> {
        self.num_runes += re.runes.len();
        let r = &re.runes;
        if re.op == Op::CharClass && r.len() == 2 && r[0] == r[1] {
            // Single rune.
            if self.maybe_concat(Some(r[0]), self.flags & !FOLD_CASE) {
                return Ok(());
            }
            re.op = Op::Literal;
            re.runes.truncate(1);
            re.flags = self.flags & !FOLD_CASE;
        } else if re.op == Op::CharClass
            && (r.len() == 4
                && r[0] == r[1]
                && r[2] == r[3]
                && simple_fold(r[0]) == r[2]
                && simple_fold(r[2]) == r[0]
                || r.len() == 2
                    && r[0] + 1 == r[1]
                    && simple_fold(r[0]) == r[1]
                    && simple_fold(r[1]) == r[0])
        {
            // Case-insensitive rune like [Aa] or [Δδ].
            if self.maybe_concat(Some(r[0]), self.flags | FOLD_CASE) {
                return Ok(());
            }
            // Rewrite as (case-insensitive) literal.
            re.op = Op::Literal;
            re.runes.truncate(1);
            re.flags = self.flags | FOLD_CASE;
        } else {
            // Incremental concatenation.
            self.maybe_concat(None, 0);
        }
        re.update();
        self.check_limits(&re)?;
        self.stack.push(re);
        Ok(())
    }

    /// Go `maybeConcat`: merges the top two stack entries if both are
    /// literals with the same case folding. If `r` is set and the merge
    /// happened, the freed top entry is reused for the literal `r`.
    fn maybe_concat(&mut self, r: Option<u32>, flags: u16) -> bool {
        let n = self.stack.len();
        if n < 2 {
            return false;
        }
        let (re2, re1) = {
            let (a, b) = self.stack.split_at_mut(n - 1);
            (&mut a[n - 2], &mut b[0])
        };
        if re1.op != Op::Literal || re2.op != Op::Literal || re1.fold() != re2.fold() {
            return false;
        }
        // Push re1 into re2.
        re2.runes.extend_from_slice(&re1.runes);
        re2.update();
        // Reuse re1 if possible.
        if let Some(r) = r {
            re1.runes.clear();
            re1.runes.push(r);
            re1.flags = flags;
            re1.update();
            return true;
        }
        self.stack.pop();
        false
    }

    /// Go `literal`.
    fn literal(&mut self, mut r: u32) -> ParseResult<()> {
        let mut re = Node::new(Op::Literal, self.flags);
        if self.flags & FOLD_CASE != 0 {
            r = min_fold_rune(r);
        }
        re.runes.push(r);
        self.push(re)
    }

    /// Go `op`.
    fn op(&mut self, op: Op) -> ParseResult<()> {
        self.push(Node::new(op, self.flags))
    }

    /// Go `repeat`: replaces the top stack element with itself repeated.
    /// `before` is where the operator starts, `after` where it ends;
    /// `last_repeat` is where the previous operator started, if the previous
    /// token was a repetition. Returns the updated `after`.
    fn repeat(
        &mut self,
        op: Op,
        min: i32,
        max: i32,
        before: usize,
        mut after: usize,
        last_repeat: Option<usize>,
    ) -> ParseResult<usize> {
        let mut flags = self.flags;
        if self.flags & PERL_X != 0 {
            if self.b.get(after) == Some(&b'?') {
                after += 1;
                flags ^= NON_GREEDY;
            }
            if let Some(last) = last_repeat {
                // In Perl it is not allowed to stack repetition operators:
                // a** is a syntax error, not a doubled star, and a++ means
                // something else entirely, which we don't support!
                return Err(self.error(ErrorCode::InvalidRepeatOp, last, after));
            }
        }
        let Some(sub) = self.stack.pop() else {
            return Err(self.error(ErrorCode::MissingRepeatArgument, before, after));
        };
        if sub.op.is_pseudo() {
            return Err(self.error(ErrorCode::MissingRepeatArgument, before, after));
        }
        let mut re = Node::new(op, flags);
        re.min = min;
        re.max = max;
        re.subs.push(sub);
        re.update();
        self.check_limits(&re)?;
        let valid = !(op == Op::Repeat && (min >= 2 || max >= 2)) || repeat_is_valid(&re, 1000);
        self.stack.push(re);
        if !valid {
            return Err(self.error(ErrorCode::InvalidRepeatSize, before, after));
        }
        Ok(after)
    }

    /// Go `concat`.
    fn concat(&mut self) -> ParseResult<()> {
        self.maybe_concat(None, 0);
        let i = self.pseudo_boundary();
        let subs = self.stack.split_off(i);
        if subs.is_empty() {
            return self.push(Node::new(Op::EmptyMatch, 0));
        }
        let re = collapse(subs, Op::Concat);
        self.push(re)
    }

    /// Go `alternate`.
    fn alternate(&mut self) -> ParseResult<()> {
        let i = self.pseudo_boundary();
        let mut subs = self.stack.split_off(i);
        // Make sure top class is clean.
        if let Some(last) = subs.last_mut() {
            clean_alt(last);
        }
        if subs.is_empty() {
            return self.push(Node::new(Op::NoMatch, 0));
        }
        let re = collapse(subs, Op::Alternate);
        self.push(re)
    }

    /// The index just above the topmost pseudo-op on the stack.
    fn pseudo_boundary(&self) -> usize {
        let mut i = self.stack.len();
        while i > 0 && !self.stack[i - 1].op.is_pseudo() {
            i -= 1;
        }
        i
    }

    /// Go `parseVerticalBar`.
    fn parse_vertical_bar(&mut self) -> ParseResult<()> {
        self.concat()?;
        // The concatenation we just parsed is on top of the stack. If it
        // sits above an opVerticalBar, swap it below (things below an
        // opVerticalBar become an alternation). Otherwise, push a new
        // vertical bar.
        if !self.swap_vertical_bar() {
            self.op(Op::VerticalBar)?;
        }
        Ok(())
    }

    /// Go `swapVerticalBar`.
    fn swap_vertical_bar(&mut self) -> bool {
        let n = self.stack.len();
        // If above and below vertical bar are literal or char class, can
        // merge into a single char class.
        if n >= 3
            && self.stack[n - 2].op == Op::VerticalBar
            && is_char_class(&self.stack[n - 1])
            && is_char_class(&self.stack[n - 3])
        {
            let mut re1 = self.stack.pop().expect("n >= 3");
            let re3 = &mut self.stack[n - 3];
            // Make re3 the more complex of the two.
            if re1.op > re3.op {
                std::mem::swap(&mut re1, re3);
            }
            merge_char_class(re3, &re1);
            return true;
        }
        if n >= 2 && self.stack[n - 2].op == Op::VerticalBar {
            if n >= 3 {
                // Now out of reach. Clean opportunistically.
                clean_alt(&mut self.stack[n - 3]);
            }
            self.stack.swap(n - 2, n - 1);
            return true;
        }
        false
    }

    /// Go `parseRightParen`.
    fn parse_right_paren(&mut self) -> ParseResult<()> {
        self.concat()?;
        if self.swap_vertical_bar() {
            self.stack.pop(); // pop vertical bar
        }
        self.alternate()?;

        let n = self.stack.len();
        if n < 2 {
            return Err(SyntaxError::new(ErrorCode::UnexpectedParen, self.s));
        }
        let re1 = self.stack.pop().expect("n >= 2");
        let mut re2 = self.stack.pop().expect("n >= 2");
        if re2.op != Op::LeftParen {
            return Err(SyntaxError::new(ErrorCode::UnexpectedParen, self.s));
        }
        // Restore flags at time of paren.
        self.flags = re2.flags;
        if re2.cap == 0 {
            // Just for grouping.
            self.push(re1)
        } else {
            re2.op = Op::Capture;
            re2.subs = vec![re1];
            self.push(re2)
        }
    }

    /// Go `parsePerlFlags`: parses a Perl flag setting or non-capturing
    /// group or both, like `(?i)` or `(?:` or `(?i:`. `t0` is at `(?`.
    fn parse_perl_flags(&mut self, t0: usize) -> ParseResult<usize> {
        let s = &self.s[t0..];
        let b = s.as_bytes();

        // Named captures: (?P<name>expr) and (?<name>expr).
        let starts_with_p = b.len() > 4 && b[2] == b'P' && b[3] == b'<';
        let starts_with_name = b.len() > 3 && b[2] == b'<';
        if starts_with_p || starts_with_name {
            let expr_start = if starts_with_name { 3 } else { 4 };
            let Some(end) = s.find('>') else {
                return Err(SyntaxError::new(ErrorCode::InvalidNamedCapture, s));
            };
            let capture = &s[..=end];
            let name = &s[expr_start..end];
            if !is_valid_capture_name(name) {
                return Err(SyntaxError::new(ErrorCode::InvalidNamedCapture, capture));
            }
            // Like ordinary capture, but named.
            self.num_cap += 1;
            let mut re = Node::new(Op::LeftParen, self.flags);
            re.cap = self.num_cap;
            self.push(re)?;
            return Ok(t0 + end + 1);
        }

        // Non-capturing group. Might also twiddle Perl flags.
        let mut t = t0 + 2;
        let mut flags = self.flags;
        let mut sign = 1;
        let mut saw_flag = false;
        while t < self.b.len() {
            let (c, next) = self.next_rune(t);
            t = next;
            match char::from_u32(c) {
                Some('i') => {
                    flags |= FOLD_CASE;
                    saw_flag = true;
                }
                Some('m') => {
                    flags &= !ONE_LINE;
                    saw_flag = true;
                }
                Some('s') => {
                    flags |= DOT_NL;
                    saw_flag = true;
                }
                Some('U') => {
                    flags |= NON_GREEDY;
                    saw_flag = true;
                }
                // Switch to negation.
                Some('-') => {
                    if sign < 0 {
                        break;
                    }
                    sign = -1;
                    // Invert flags so that | above turn into &^ and vice
                    // versa. We'll invert flags again before using it below.
                    flags = !flags;
                    saw_flag = false;
                }
                // End of flags, starting group or not.
                Some(c @ (':' | ')')) => {
                    if sign < 0 {
                        if !saw_flag {
                            break;
                        }
                        flags = !flags;
                    }
                    if c == ':' {
                        // Open new group.
                        self.op(Op::LeftParen)?;
                    }
                    self.flags = flags;
                    return Ok(t);
                }
                _ => break,
            }
        }
        Err(self.error(ErrorCode::InvalidPerlOp, t0, t))
    }

    /// Go `parseRepeat`: parses `{min}` (max = min), `{min,}` (max = -1), or
    /// `{min,max}` at `t`. Returns None if the text is not of that form, and
    /// min = -1 if it has the form but a number is too big.
    fn parse_repeat(&self, t: usize) -> Option<(i32, i32, usize)> {
        let b = self.b;
        if b.get(t) != Some(&b'{') {
            return None;
        }
        let (mut min, mut t) = parse_int(b, t + 1)?;
        let max;
        match b.get(t) {
            None => return None,
            Some(&b',') => {
                t += 1;
                match b.get(t) {
                    None => return None,
                    Some(&b'}') => max = -1,
                    Some(_) => {
                        let (m, rest) = parse_int(b, t)?;
                        t = rest;
                        max = m;
                        if max < 0 {
                            // parseInt found too big a number.
                            min = -1;
                        }
                    }
                }
            }
            Some(_) => max = min,
        }
        if b.get(t) != Some(&b'}') {
            return None;
        }
        Some((min, max, t + 1))
    }

    /// Handles a `\` in the main loop of Go `parse`.
    fn parse_backslash(&mut self, t: usize) -> ParseResult<usize> {
        if self.flags & PERL_X != 0 && t + 1 < self.b.len() {
            match self.b[t + 1] {
                b'A' => {
                    self.op(Op::BeginText)?;
                    return Ok(t + 2);
                }
                b'b' => {
                    self.op(Op::WordBoundary)?;
                    return Ok(t + 2);
                }
                b'B' => {
                    self.op(Op::NoWordBoundary)?;
                    return Ok(t + 2);
                }
                b'C' => {
                    // Any byte; not supported.
                    return Err(self.error(ErrorCode::InvalidEscape, t, t + 2));
                }
                b'Q' => {
                    // \Q ... \E: the ... is always literals. (Unreachable
                    // after check_regex_dialect, ported for completeness.)
                    let rest = &self.s[t + 2..];
                    let (lit, after) = match rest.find(r"\E") {
                        Some(i) => (&rest[..i], t + 2 + i + 2),
                        None => (rest, self.b.len()),
                    };
                    for c in lit.chars() {
                        self.literal(u32::from(c))?;
                    }
                    return Ok(after);
                }
                b'z' => {
                    self.op(Op::EndText)?;
                    return Ok(t + 2);
                }
                _ => {}
            }
        }

        // Look for Unicode character group like \p{Han}.
        if t + 1 < self.b.len() && matches!(self.b[t + 1], b'p' | b'P') {
            let mut class = Vec::new();
            if let Some(rest) = self.parse_unicode_class(t, &mut class)? {
                let mut re = Node::new(Op::CharClass, self.flags);
                re.runes = class;
                self.push(re)?;
                return Ok(rest);
            }
        }

        // Perl character class escape.
        let mut class = Vec::new();
        if let Some(rest) = self.parse_perl_class_escape(t, &mut class) {
            let mut re = Node::new(Op::CharClass, self.flags);
            re.runes = class;
            self.push(re)?;
            return Ok(rest);
        }

        // Ordinary single-character escape.
        let (c, rest) = self.parse_escape(t)?;
        self.literal(c)?;
        Ok(rest)
    }

    /// Go `parseEscape`: parses the escape sequence at `t0` (a `\`) and
    /// returns the rune and the position after it.
    fn parse_escape(&self, t0: usize) -> ParseResult<(u32, usize)> {
        let mut t = t0 + 1;
        if t >= self.b.len() {
            return Err(SyntaxError::new(ErrorCode::TrailingBackslash, ""));
        }
        let (c, next) = self.next_rune(t);
        t = next;
        let bad = |t: usize| Err(self.error(ErrorCode::InvalidEscape, t0, t));
        let Some(ch) = char::from_u32(c) else {
            return bad(t);
        };
        match ch {
            // Octal escapes.
            '1'..='7' | '0' => {
                // Single non-zero digit is a backreference; not supported.
                if ch != '0' && !matches!(self.b.get(t), Some(b'0'..=b'7')) {
                    return bad(t);
                }
                // Consume up to three octal digits; already have one.
                let mut r = c - u32::from('0');
                for _ in 1..3 {
                    match self.b.get(t) {
                        Some(&d @ b'0'..=b'7') => {
                            r = r * 8 + u32::from(d - b'0');
                            t += 1;
                        }
                        _ => break,
                    }
                }
                Ok((r, t))
            }
            // Hexadecimal escapes.
            'x' => {
                if t >= self.b.len() {
                    return bad(t);
                }
                let (c, next) = self.next_rune(t);
                t = next;
                if c == u32::from('{') {
                    // Any number of digits in braces. We require only hex
                    // digits, and at least one.
                    let mut nhex = 0;
                    let mut r: u32 = 0;
                    loop {
                        if t >= self.b.len() {
                            return bad(t);
                        }
                        let (c, next) = self.next_rune(t);
                        t = next;
                        if c == u32::from('}') {
                            break;
                        }
                        let Some(v) = unhex(c) else { return bad(t) };
                        r = r * 16 + v;
                        if r > MAX_RUNE {
                            return bad(t);
                        }
                        nhex += 1;
                    }
                    if nhex == 0 {
                        return bad(t);
                    }
                    return Ok((r, t));
                }
                // Easy case: two hex digits.
                let x = unhex(c);
                let (c, next) = self.next_rune(t);
                t = next;
                match (x, unhex(c)) {
                    (Some(x), Some(y)) => Ok((x * 16 + y, t)),
                    _ => bad(t),
                }
            }
            // C escapes. There is no case 'b', to avoid misparsing the Perl
            // word-boundary \b as the C backspace \b.
            'a' => Ok((0x07, t)),
            'f' => Ok((0x0C, t)),
            'n' => Ok((0x0A, t)),
            'r' => Ok((0x0D, t)),
            't' => Ok((0x09, t)),
            'v' => Ok((0x0B, t)),
            // Escaped non-word characters are always themselves.
            _ if ch.is_ascii() && !ch.is_ascii_alphanumeric() => Ok((c, t)),
            _ => bad(t),
        }
    }

    /// Go `parseClassChar`.
    fn parse_class_char(&self, t: usize, whole_class: usize) -> ParseResult<(u32, usize)> {
        if t >= self.b.len() {
            return Err(SyntaxError::new(
                ErrorCode::MissingBracket,
                &self.s[whole_class..],
            ));
        }
        // Allow regular escape sequences even though many need not be
        // escaped in this context.
        if self.b[t] == b'\\' {
            return self.parse_escape(t);
        }
        Ok(self.next_rune(t))
    }

    /// Go `parsePerlClassEscape`: appends a Perl class escape like `\d` at
    /// `t` to `r` and returns the position after it.
    fn parse_perl_class_escape(&self, t: usize, r: &mut Vec<u32>) -> Option<usize> {
        if self.flags & PERL_X == 0 || t + 2 > self.b.len() || self.b[t] != b'\\' {
            return None;
        }
        let (class, negated) = match self.b[t + 1] {
            b'd' => (PERL_DIGIT, false),
            b'D' => (PERL_DIGIT, true),
            b's' => (PERL_SPACE, false),
            b'S' => (PERL_SPACE, true),
            b'w' => (PERL_WORD, false),
            b'W' => (PERL_WORD, true),
            _ => return None,
        };
        self.append_group(r, class, negated);
        Some(t + 2)
    }

    /// Go `parseNamedClass`: appends a POSIX class like `[:alnum:]` at `t`
    /// to `r` and returns the position after it.
    fn parse_named_class(&self, t: usize, r: &mut Vec<u32>) -> ParseResult<Option<usize>> {
        let b = &self.b[t..];
        if b.len() < 2 || b[0] != b'[' || b[1] != b':' {
            return Ok(None);
        }
        let Some(i) = find(&b[2..], b":]") else {
            return Ok(None);
        };
        let end = t + 2 + i + 2;
        let name = &self.s[t..end];
        let Some((class, negated)) = posix_group(name) else {
            return Err(SyntaxError::new(ErrorCode::InvalidCharRange, name));
        };
        self.append_group(r, class, negated);
        Ok(Some(end))
    }

    /// Go `appendGroup`.
    fn append_group(&self, r: &mut Vec<u32>, class: &[u32], negated: bool) {
        if self.flags & FOLD_CASE == 0 {
            if negated {
                append_negated_class(r, class);
            } else {
                append_class(r, class);
            }
        } else {
            let mut tmp = Vec::new();
            append_folded_class(&mut tmp, class);
            clean_class(&mut tmp);
            if negated {
                append_negated_class(r, &tmp);
            } else {
                append_class(r, &tmp);
            }
        }
    }

    /// Go `parseUnicodeClass`: appends a Unicode class like `\p{Han}` at
    /// `t0` to `r` and returns the position after it, or None if there is
    /// no `\p` or `\P` at `t0`.
    fn parse_unicode_class(&self, t0: usize, r: &mut Vec<u32>) -> ParseResult<Option<usize>> {
        let s = &self.s[t0..];
        let b = s.as_bytes();
        if self.flags & UNICODE_GROUPS == 0
            || b.len() < 2
            || b[0] != b'\\'
            || b[1] != b'p' && b[1] != b'P'
        {
            return Ok(None);
        }

        // Committed to parse or return error.
        let mut sign = if b[1] == b'P' { -1 } else { 1 };
        let (c, next) = self.next_rune(t0 + 2);
        let (seq, mut name, rest);
        if c != u32::from('{') {
            // Single-letter name.
            seq = &self.s[t0..next];
            name = &seq[2..];
            rest = next;
        } else {
            // Name is in braces.
            let Some(end) = s.find('}') else {
                return Err(SyntaxError::new(ErrorCode::InvalidCharRange, s));
            };
            seq = &s[..=end];
            name = &s[3..end];
            rest = t0 + end + 1;
        }

        // Group can have leading negation too. \p{^Han} == \P{Han}.
        if let Some(n) = name.strip_prefix('^') {
            sign = -sign;
            name = n;
        }

        let Some(class) = unicode_table(name) else {
            return Err(SyntaxError::new(ErrorCode::InvalidCharRange, seq));
        };
        if class.negate {
            sign = -sign;
        }

        match class.fold {
            Some(fold) if self.flags & FOLD_CASE != 0 => {
                // Merge and clean tab and fold in a temporary buffer.
                let mut tmp = Vec::new();
                append_table(&mut tmp, class.table);
                append_table(&mut tmp, fold);
                clean_class(&mut tmp);
                if sign > 0 {
                    append_class(r, &tmp);
                } else {
                    append_negated_class(r, &tmp);
                }
            }
            _ => {
                if sign > 0 {
                    append_table(r, class.table);
                } else {
                    append_negated_table(r, class.table);
                }
            }
        }
        Ok(Some(rest))
    }

    /// Go `parseClass`: parses the character class at `t0` (a `[`), pushes
    /// it, and returns the position after it.
    fn parse_class(&mut self, t0: usize) -> ParseResult<usize> {
        let mut t = t0 + 1; // chop [
        let mut re = Node::new(Op::CharClass, self.flags);
        let mut class = Vec::new();

        let mut negated = false;
        if self.b.get(t) == Some(&b'^') {
            negated = true;
            t += 1;
            // If character class does not match \n, add it here, so that
            // negation later will do the right thing.
            if self.flags & CLASS_NL == 0 {
                class.extend_from_slice(&[0x0A, 0x0A]);
            }
        }

        let mut first = true; // ] and - are okay as first char in class
        while t >= self.b.len() || self.b[t] != b']' || first {
            // POSIX: - is only okay unescaped as first or last in class.
            // Perl: - is okay anywhere (PerlX is always set here).
            first = false;

            // Look for POSIX [:alnum:] etc.
            if self.b.len() - t > 2
                && self.b[t] == b'['
                && self.b[t + 1] == b':'
                && let Some(next) = self.parse_named_class(t, &mut class)?
            {
                t = next;
                continue;
            }

            // Look for Unicode character group like \p{Han}.
            if let Some(next) = self.parse_unicode_class(t, &mut class)? {
                t = next;
                continue;
            }

            // Look for Perl character class symbols (extension).
            if let Some(next) = self.parse_perl_class_escape(t, &mut class) {
                t = next;
                continue;
            }

            // Single character or simple range.
            let rng = t;
            let (lo, next) = self.parse_class_char(t, t0)?;
            t = next;
            let mut hi = lo;
            // [a-] means (a|-) so check for final ].
            if self.b.len() - t >= 2 && self.b[t] == b'-' && self.b[t + 1] != b']' {
                t += 1;
                let (h, next) = self.parse_class_char(t, t0)?;
                t = next;
                hi = h;
                if hi < lo {
                    return Err(self.error(ErrorCode::InvalidCharRange, rng, t));
                }
            }
            if self.flags & FOLD_CASE == 0 {
                append_range(&mut class, lo, hi);
            } else {
                append_folded_range(&mut class, lo, hi);
            }
        }
        t += 1; // chop ]

        clean_class(&mut class);
        if negated {
            class = negate_class(&class);
        }
        re.runes = class;
        self.push(re)?;
        Ok(t)
    }
}

/// Go `parseInt`: parses a decimal integer at `t` (no leading zeros).
/// Returns -1 for numbers of 1e8 or more.
fn parse_int(b: &[u8], t: usize) -> Option<(i32, usize)> {
    if !b.get(t).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    // Disallow leading zeros.
    if b[t] == b'0' && b.get(t + 1).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let end = skip_digits(b, t);
    let mut n: i32 = 0;
    for &d in &b[t..end] {
        // Avoid overflow.
        if n >= 100_000_000 {
            n = -1;
            break;
        }
        n = n * 10 + i32::from(d - b'0');
    }
    Some((n, end))
}

/// Go `isValidCaptureName`: `[A-Za-z0-9_]+`.
fn is_valid_capture_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|c| c == b'_' || c.is_ascii_alphanumeric())
}

fn unhex(c: u32) -> Option<u32> {
    char::from_u32(c)?.to_digit(16)
}

/// Go `repeatIsValid`: whether the repetition `re` combined with any inner
/// repetitions does not exceed `n` copies of the innermost thing.
fn repeat_is_valid(re: &Node, mut n: i32) -> bool {
    if re.op == Op::Repeat {
        let mut m = re.max;
        if m == 0 {
            return true;
        }
        if m < 0 {
            m = re.min;
        }
        if m > n {
            return false;
        }
        if m > 0 {
            n /= m;
        }
    }
    re.subs.iter().all(|sub| repeat_is_valid(sub, n))
}

/// Go `collapse`: applies `op` to `subs`, hoisting subexpressions that are
/// already `op`. Go also factors common prefixes out of alternations; that
/// does not change what the expression matches and is not ported.
fn collapse(subs: Vec<Node>, op: Op) -> Node {
    if subs.len() == 1 {
        return subs.into_iter().next().expect("one element");
    }
    let mut re = Node::new(op, 0);
    for sub in subs {
        if sub.op == op {
            re.subs.extend(sub.subs);
        } else {
            re.subs.push(sub);
        }
    }
    re.update();
    re
}

/// Go `cleanAlt`.
fn clean_alt(re: &mut Node) {
    if re.op == Op::CharClass {
        clean_class(&mut re.runes);
        if re.runes == [0, MAX_RUNE] {
            re.runes.clear();
            re.op = Op::AnyChar;
        } else if re.runes == [0, 0x09, 0x0B, MAX_RUNE] {
            re.runes.clear();
            re.op = Op::AnyCharNotNL;
        }
        re.update();
    }
}

/// Go `isCharClass`.
fn is_char_class(re: &Node) -> bool {
    re.op == Op::Literal && re.runes.len() == 1
        || matches!(re.op, Op::CharClass | Op::AnyCharNotNL | Op::AnyChar)
}

/// Go `matchRune`.
fn match_rune(re: &Node, r: u32) -> bool {
    match re.op {
        Op::Literal => re.runes.len() == 1 && re.runes[0] == r,
        Op::CharClass => re.runes.chunks_exact(2).any(|p| p[0] <= r && r <= p[1]),
        Op::AnyCharNotNL => r != 0x0A,
        Op::AnyChar => true,
        _ => false,
    }
}

/// Go `mergeCharClass`: makes dst = dst|src, where dst.op >= src.op.
fn merge_char_class(dst: &mut Node, src: &Node) {
    match dst.op {
        Op::AnyChar => {} // src doesn't add anything
        Op::AnyCharNotNL => {
            // src might add \n
            if match_rune(src, 0x0A) {
                dst.op = Op::AnyChar;
            }
        }
        Op::CharClass => {
            // src is simpler, so either literal or char class
            if src.op == Op::Literal {
                append_literal(&mut dst.runes, src.runes[0], src.flags);
            } else {
                append_class(&mut dst.runes, &src.runes);
            }
        }
        // Both literal.
        Op::Literal if !(src.runes[0] == dst.runes[0] && src.flags == dst.flags) => {
            let r = dst.runes[0];
            dst.op = Op::CharClass;
            dst.runes.clear();
            append_literal(&mut dst.runes, r, dst.flags);
            append_literal(&mut dst.runes, src.runes[0], src.flags);
        }
        _ => {}
    }
    dst.update();
}

// Character class helpers (Go parse.go). A class is a flat list of lo, hi
// pairs.

/// Go `cleanClass`: sorts the ranges, merges them, and removes duplicates.
fn clean_class(r: &mut Vec<u32>) {
    let mut pairs: Vec<(u32, u32)> = r.chunks_exact(2).map(|p| (p[0], p[1])).collect();
    // Sort by lo increasing, hi decreasing to break ties.
    pairs.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    r.clear();
    for (lo, hi) in pairs {
        if let Some(last) = r.last_mut()
            && lo <= *last + 1
        {
            // Merge with previous range.
            if hi > *last {
                *last = hi;
            }
            continue;
        }
        r.push(lo);
        r.push(hi);
    }
}

/// Go `appendLiteral`.
fn append_literal(r: &mut Vec<u32>, x: u32, flags: u16) {
    if flags & FOLD_CASE != 0 {
        append_folded_range(r, x, x);
    } else {
        append_range(r, x, x);
    }
}

/// Go `appendRange`: appends lo-hi, expanding the last or next-to-last range
/// if it overlaps or abuts.
fn append_range(r: &mut Vec<u32>, lo: u32, hi: u32) {
    let n = r.len();
    for i in [2, 4] {
        if n >= i {
            let (rlo, rhi) = (r[n - i], r[n - i + 1]);
            if lo <= rhi + 1 && rlo <= hi + 1 {
                if lo < rlo {
                    r[n - i] = lo;
                }
                if hi > rhi {
                    r[n - i + 1] = hi;
                }
                return;
            }
        }
    }
    r.push(lo);
    r.push(hi);
}

/// Go `appendFoldedRange`: appends lo-hi and its case folding-equivalent
/// runes.
fn append_folded_range(r: &mut Vec<u32>, mut lo: u32, mut hi: u32) {
    // Optimizations.
    if lo <= MIN_FOLD && hi >= MAX_FOLD {
        // Range is full: folding can't add more.
        return append_range(r, lo, hi);
    }
    if hi < MIN_FOLD || lo > MAX_FOLD {
        // Range is outside folding possibilities.
        return append_range(r, lo, hi);
    }
    if lo < MIN_FOLD {
        // [lo, minFold-1] needs no folding.
        append_range(r, lo, MIN_FOLD - 1);
        lo = MIN_FOLD;
    }
    if hi > MAX_FOLD {
        // [maxFold+1, hi] needs no folding.
        append_range(r, MAX_FOLD + 1, hi);
        hi = MAX_FOLD;
    }
    // Brute force. Depend on append_range to coalesce ranges on the fly.
    for c in lo..=hi {
        append_range(r, c, c);
        let mut f = simple_fold(c);
        while f != c {
            append_range(r, f, f);
            f = simple_fold(f);
        }
    }
}

/// Go `appendClass` (x is clean).
fn append_class(r: &mut Vec<u32>, x: &[u32]) {
    for p in x.chunks_exact(2) {
        append_range(r, p[0], p[1]);
    }
}

/// Go `appendFoldedClass`.
fn append_folded_class(r: &mut Vec<u32>, x: &[u32]) {
    for p in x.chunks_exact(2) {
        append_folded_range(r, p[0], p[1]);
    }
}

/// Go `appendNegatedClass` (x is clean).
fn append_negated_class(r: &mut Vec<u32>, x: &[u32]) {
    let mut next_lo = 0;
    for p in x.chunks_exact(2) {
        let (lo, hi) = (p[0], p[1]);
        if lo > next_lo {
            append_range(r, next_lo, lo - 1);
        }
        next_lo = hi + 1;
    }
    if next_lo <= MAX_RUNE {
        append_range(r, next_lo, MAX_RUNE);
    }
}

/// Go `appendTable`; the generated tables are already sorted, merged
/// ranges.
fn append_table(r: &mut Vec<u32>, x: &[(u32, u32)]) {
    for &(lo, hi) in x {
        append_range(r, lo, hi);
    }
}

/// Go `appendNegatedTable`.
fn append_negated_table(r: &mut Vec<u32>, x: &[(u32, u32)]) {
    let mut next_lo = 0;
    for &(lo, hi) in x {
        if lo > next_lo {
            append_range(r, next_lo, lo - 1);
        }
        next_lo = hi + 1;
    }
    if next_lo <= MAX_RUNE {
        append_range(r, next_lo, MAX_RUNE);
    }
}

/// Go `negateClass` (r is clean).
fn negate_class(r: &[u32]) -> Vec<u32> {
    let mut out = Vec::with_capacity(r.len() + 2);
    let mut next_lo = 0;
    for p in r.chunks_exact(2) {
        let (lo, hi) = (p[0], p[1]);
        if lo > next_lo {
            out.push(next_lo);
            out.push(lo - 1);
        }
        next_lo = hi + 1;
    }
    if next_lo <= MAX_RUNE {
        out.push(next_lo);
        out.push(MAX_RUNE);
    }
    out
}

/// Go `unicode.SimpleFold`.
fn simple_fold(r: u32) -> u32 {
    match SIMPLE_FOLD.binary_search_by_key(&r, |&(from, _)| from) {
        Ok(i) => SIMPLE_FOLD[i].1,
        Err(_) => r,
    }
}

/// Go `minFoldRune`: the minimum rune fold-equivalent to r.
fn min_fold_rune(r: u32) -> u32 {
    if !(MIN_FOLD..=MAX_FOLD).contains(&r) {
        return r;
    }
    let mut m = r;
    let mut f = simple_fold(r);
    while f != r {
        m = m.min(f);
        f = simple_fold(f);
    }
    m
}

/// Go `canonicalName`: a leading uppercase letter, then lowercase letters,
/// without underscores, spaces, and hyphens.
fn canonical_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut first = true;
    for c in name.bytes() {
        match c {
            b'_' | b'-' | b' ' => continue,
            _ if first => {
                first = false;
                out.push(c.to_ascii_uppercase() as char);
            }
            _ => out.push(c.to_ascii_lowercase() as char),
        }
    }
    out
}

/// Go `unicodeTable`.
fn unicode_table(name: &str) -> Option<&'static unicode_names::UnicodeClass> {
    let name = canonical_name(name);
    GO_UNICODE_NAMES
        .binary_search_by(|class| class.name.cmp(&name))
        .ok()
        .map(|i| &GO_UNICODE_NAMES[i])
}

/// Whether `c` belongs to the Go Unicode class `name` (Go's tables, so the
/// Unicode version matches Go's). Unknown names contain nothing.
pub(crate) fn in_go_class(name: &str, c: char) -> bool {
    unicode_table(name).is_some_and(|class| {
        let r = c as u32;
        let i = class.table.partition_point(|&(_, hi)| hi < r);
        let inside = i < class.table.len() && class.table[i].0 <= r;
        inside != class.negate
    })
}

// Go perl_groups.go.
const PERL_DIGIT: &[u32] = &[0x30, 0x39];
const PERL_SPACE: &[u32] = &[0x09, 0x0A, 0x0C, 0x0D, 0x20, 0x20];
const PERL_WORD: &[u32] = &[0x30, 0x39, 0x41, 0x5A, 0x5F, 0x5F, 0x61, 0x7A];

/// Go `posixGroup`: the class for `[:name:]` or `[:^name:]` and whether it
/// is negated.
fn posix_group(name: &str) -> Option<(&'static [u32], bool)> {
    let inner = name.strip_prefix("[:")?.strip_suffix(":]")?;
    let (negated, inner) = match inner.strip_prefix('^') {
        Some(rest) => (true, rest),
        None => (false, inner),
    };
    let class: &'static [u32] = match inner {
        "alnum" => &[0x30, 0x39, 0x41, 0x5A, 0x61, 0x7A],
        "alpha" => &[0x41, 0x5A, 0x61, 0x7A],
        "ascii" => &[0x00, 0x7F],
        "blank" => &[0x09, 0x09, 0x20, 0x20],
        "cntrl" => &[0x00, 0x1F, 0x7F, 0x7F],
        "digit" => &[0x30, 0x39],
        "graph" => &[0x21, 0x7E],
        "lower" => &[0x61, 0x7A],
        "print" => &[0x20, 0x7E],
        "punct" => &[0x21, 0x2F, 0x3A, 0x40, 0x5B, 0x60, 0x7B, 0x7E],
        "space" => &[0x09, 0x0D, 0x20, 0x20],
        "upper" => &[0x41, 0x5A],
        "word" => PERL_WORD,
        "xdigit" => &[0x30, 0x39, 0x41, 0x46, 0x61, 0x66],
        _ => return None,
    };
    Some((class, negated))
}

// ---------------------------------------------------------------------------
// Printing Go's parse tree as a `regex` crate pattern.

/// Matches nothing.
const NO_MATCH: &str = r"[^\x{0}-\x{10FFFF}]";

/// Appends `re` to `out` in `regex` crate syntax. Classes and case folding
/// are spelled out as explicit ranges; no `regex` flag other than scoped
/// `(?s:.)`, `(?m:^)`, `(?m:$)`, and `(?-u:\b)` is used.
fn emit(re: &Node, out: &mut String) {
    match re.op {
        Op::NoMatch => out.push_str(NO_MATCH),
        Op::EmptyMatch => out.push_str("(?:)"),
        Op::Literal => {
            for &r in &re.runes {
                if re.fold() {
                    emit_class(&fold_orbit(r), out);
                } else if char::from_u32(r).is_some() {
                    emit_char(r, out);
                } else {
                    // A surrogate code point never matches a Go string.
                    out.push_str(NO_MATCH);
                }
            }
        }
        Op::CharClass => emit_class(&re.runes, out),
        Op::AnyCharNotNL => out.push_str(r"[^\n]"),
        Op::AnyChar => out.push_str("(?s:.)"),
        Op::BeginLine => out.push_str("(?m:^)"),
        Op::EndLine => out.push_str("(?m:$)"),
        Op::BeginText => out.push_str(r"\A"),
        Op::EndText => out.push_str(r"\z"),
        // Go's \b and \B are ASCII word boundaries, tested only between
        // runes. (?-u:\b) cannot hold inside a UTF-8 sequence, but (?-u:\B)
        // can, and the regex crate's is_match then misses other matches (it
        // reports no match for (?-u:\B)|\x{212A}(?-u:\b) on "a\u{212A}b",
        // where find succeeds). (?:\b|\B) holds exactly at char boundaries.
        Op::WordBoundary => out.push_str(r"(?-u:\b)"),
        Op::NoWordBoundary => out.push_str(r"(?-u:\B)(?:\b|\B)"),
        Op::Capture => {
            out.push('(');
            emit(&re.subs[0], out);
            out.push(')');
        }
        Op::Star | Op::Plus | Op::Quest | Op::Repeat => {
            out.push_str("(?:");
            emit(&re.subs[0], out);
            out.push(')');
            match re.op {
                Op::Star => out.push('*'),
                Op::Plus => out.push('+'),
                Op::Quest => out.push('?'),
                _ if re.max == re.min => {
                    let _ = write!(out, "{{{}}}", re.min);
                }
                _ if re.max == -1 => {
                    let _ = write!(out, "{{{},}}", re.min);
                }
                _ => {
                    let _ = write!(out, "{{{},{}}}", re.min, re.max);
                }
            }
            if re.flags & NON_GREEDY != 0 {
                out.push('?');
            }
        }
        Op::Concat => {
            for sub in &re.subs {
                emit(sub, out);
            }
        }
        Op::Alternate => {
            out.push_str("(?:");
            for (i, sub) in re.subs.iter().enumerate() {
                if i > 0 {
                    out.push('|');
                }
                emit(sub, out);
            }
            out.push(')');
        }
        // The parser never leaves pseudo-ops in a finished tree.
        Op::LeftParen | Op::VerticalBar => {}
    }
}

/// The runes Go's case-folded literal r matches: its unicode.SimpleFold
/// orbit, as a clean class.
fn fold_orbit(r: u32) -> Vec<u32> {
    let mut class = vec![r, r];
    let mut f = simple_fold(r);
    while f != r {
        class.push(f);
        class.push(f);
        f = simple_fold(f);
    }
    clean_class(&mut class);
    class
}

/// Appends a bracketed class for the flat ranges `r`, without surrogates
/// (which never match a Go string, and which `regex` cannot express).
fn emit_class(r: &[u32], out: &mut String) {
    let start = out.len();
    out.push('[');
    let mut empty = true;
    for p in r.chunks_exact(2) {
        let (lo, hi) = (p[0], p[1].min(MAX_RUNE));
        for (lo, hi) in [(lo, hi.min(0xD7FF)), (lo.max(0xE000), hi)] {
            if lo > hi {
                continue;
            }
            empty = false;
            emit_char(lo, out);
            if hi > lo {
                out.push('-');
                emit_char(hi, out);
            }
        }
    }
    if empty {
        out.truncate(start);
        out.push_str(NO_MATCH);
    } else {
        out.push(']');
    }
}

/// Appends the scalar value r as a literal that means r both inside and
/// outside a bracketed class.
fn emit_char(r: u32, out: &mut String) {
    match char::from_u32(r) {
        Some(c) if c.is_ascii_alphanumeric() => out.push(c),
        _ => {
            let _ = write!(out, r"\x{{{r:X}}}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compile(raw: &str) -> Result<regex::Regex, String> {
        compile_literal(raw)
    }

    #[test]
    fn matches_go_on_differential_corpus() {
        let mut failures = Vec::new();
        for &(raw, want) in GO_CASES {
            match (compile(raw), want) {
                (Ok(re), Ok(bits)) => {
                    let got: String = HAYSTACKS
                        .iter()
                        .map(|h| if re.is_match(h) { '1' } else { '0' })
                        .collect();
                    if got != bits {
                        failures.push(format!("{raw}: matches {got}, Go {bits}"));
                    }
                }
                (Err(err), Err(go)) if err == go => {}
                (got, want) => {
                    failures.push(format!("{raw}: got {:?}, Go {want:?}", got.map(|_| ())))
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn kelvin_and_long_s_fold_into_ascii_word_class() {
        // Go case folds Perl and POSIX classes under (?i): K (U+212A KELVIN
        // SIGN) folds to k and ſ (U+017F LATIN SMALL LETTER LONG S) to s.
        for raw in [
            r"/\w/i",
            r"/(?i)\w/",
            r"/[\w]/i",
            r"/[[:word:]]/i",
            r"/[[:alpha:]]/i",
            r"/[a-z]/i",
        ] {
            let re = compile(raw).unwrap();
            assert!(re.is_match("\u{212A}") && re.is_match("\u{17F}"), "{raw}");
        }
        assert!(compile("/k/i").unwrap().is_match("\u{212A}"));
        assert!(compile("/s/i").unwrap().is_match("\u{17F}"));
        assert!(!compile(r"/\w/").unwrap().is_match("\u{212A}"));
        assert!(!compile(r"/\W/i").unwrap().is_match("\u{212A}"));
        assert!(!compile(r"/[^\w]/i").unwrap().is_match("\u{17F}"));
        assert!(compile(r"/\W/").unwrap().is_match("\u{212A}"));
        assert!(!compile(r"/\w/").unwrap().is_match("\u{E9}"));
    }

    #[test]
    fn ascii_word_boundaries_hold_only_between_runes() {
        // Go 1.27.1 MatchString results for each haystack.
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
        ];
        for (raw, want) in cases {
            let re = compile(raw).unwrap();
            let got: String = haystacks
                .iter()
                .map(|h| if re.is_match(h) { '1' } else { '0' })
                .collect();
            assert_eq!(got, want, "{raw}");
        }
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

    /// Patterns near Go's limits, with Go 1.27.1's results. Run on a large
    /// stack: the `regex` crate compiles deep nesting recursively, which
    /// needs more than the default test thread stack in debug builds.
    #[test]
    fn limits_match_go() {
        fn nest(open: &str, inner: &str, close: &str, n: usize) -> String {
            format!("/{}{inner}{}/", open.repeat(n), close.repeat(n))
        }
        let cases: Vec<(String, Result<(), &str>)> = vec![
            (nest("(", "a", ")", 999), Ok(())),
            (
                nest("(", "a", ")", 1000),
                Err("expression nests too deeply"),
            ),
            (nest("(?:", "a", ")", 5000), Ok(())),
            (nest("(?:", "a", ")*", 999), Ok(())),
            (
                nest("(?:", "a", ")*", 1000),
                Err("expression nests too deeply"),
            ),
            (nest("(?:b|c(?:", "a", ")?)", 333), Ok(())),
            (
                nest("(?:b|c(?:", "a", ")?)", 334),
                Err("expression nests too deeply"),
            ),
            // Adjacent literals merge into one node, so this is shallow in Go.
            (
                format!("/(?:x{})/", nest("(?:y", "", ")", 1000).trim_matches('/')),
                Ok(()),
            ),
            (format!("/(?:{}){{1000}}/", "a".repeat(3355)), Ok(())),
            (
                format!("/(?:{}){{1000}}/", "a".repeat(3356)),
                Err("expression too large"),
            ),
            (format!("/{}/", "(".repeat(1000)), Err("missing closing )")),
            (format!("/{}/", "a".repeat(100_000)), Ok(())),
        ];
        std::thread::Builder::new()
            .stack_size(256 << 20)
            .spawn(move || {
                for (raw, want) in cases {
                    let got = compile(&raw).map(|_| ());
                    match (got, want) {
                        (Ok(()), Ok(())) => {}
                        (Err(err), Err(code)) => {
                            let prefix = format!("error parsing regexp: {code}: `");
                            assert!(err.starts_with(&prefix), "{:.40}: {err:.80}", raw);
                        }
                        (got, want) => panic!("{:.40}: got {got:.80?}, Go {want:?}", raw),
                    }
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn go_tables_are_sorted() {
        assert!(GO_UNICODE_NAMES.windows(2).all(|w| w[0].name < w[1].name));
        assert!(SIMPLE_FOLD.windows(2).all(|w| w[0].0 < w[1].0));
        // Go checks minFold and maxFold against the fold tables the same way.
        assert_eq!(SIMPLE_FOLD.first().unwrap().0, MIN_FOLD);
        assert_eq!(SIMPLE_FOLD.last().unwrap().0, MAX_FOLD);
        assert_eq!(canonical_name("old_italic"), "Olditalic");
        assert_eq!(canonical_name("L-c"), "Lc");
        assert!(unicode_table("greek").is_some());
        assert!(unicode_table("Greek=1").is_none());
    }

    /// Haystacks for the differential corpus.
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
        r#"\"#,
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
        "\u{a7ce}",
        "\u{a7cf}",
        "\u{88f}",
        "\u{378}",
        "\u{e000}",
        "\u{10940}",
        "stra\u{df}e",
        "STRASSE",
    ];

    /// Go 1.27.1 `parseRegex` results: for each regex token, either the
    /// `MatchString` result for every haystack (1 = match) or the error.
    const GO_CASES: &[(&str, Result<&str, &str>)] = &[
        (
            "/curl/i",
            Ok("00000000000000000000000000000000000000000001000000000000000000"),
        ),
        (
            r#"/example\.com$/"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "|^/usr/bin/|",
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/^x/",
            Ok("00000000100000000100000000000000000000000000000000000000000000"),
        ),
        (
            "/^d/",
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/^z/",
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/^10\./"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/^https:/",
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/^192\./"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/^01:23/",
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/x/i",
            Ok("00000000100000000100000000000000000000000000000000000000000000"),
        ),
        (
            "/x/",
            Ok("00000000100000000100000000000000000000000000000000000000000000"),
        ),
        (
            "/gl=se$/",
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "|some/path/here|",
            Ok("00000000000000000000000000000000000000000000100000000000000000"),
        ),
        (
            "|curl|i",
            Ok("00000000000000000000000000000000000000000001000000000000000000"),
        ),
        (
            "/curl/",
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/a.b/s",
            Ok("00000000000000001100000000000000000000010000010000000000000000"),
        ),
        (
            "/^b$/m",
            Ok("00010000000000001100000000000000000000000000000000000000000000"),
        ),
        (
            "/^A.B$/ims",
            Ok("00000000000000001100000000000000000000010000010000000000000000"),
        ),
        (
            r#"/\<a/"#,
            Err(r#"\< is not supported; use \b for word boundaries"#),
        ),
        (
            r#"/a\>/"#,
            Err(r#"\> is not supported; use \b for word boundaries"#),
        ),
        (
            r#"/\Qa.b\E/"#,
            Err(r#"\Q...\E is not supported; escape each character instead"#),
        ),
        (
            r#"/\0/"#,
            Err(r#"\0 is not supported; use \x{...} for character codes"#),
        ),
        (
            r#"/\12/"#,
            Err(r#"\1 is not supported; use \x{...} for character codes"#),
        ),
        (r#"/\b{start}a/"#, Err(r#"\b{...} is not supported"#)),
        (
            r#"/\p{^L}/"#,
            Err(r#"\p{^...} is not supported; use \P{...} to negate a Unicode class"#),
        ),
        ("/a{,3}/", Err("{,n} is not supported; use {0,n}")),
        (
            r#"/[\d-z]/"#,
            Err(r#"\d cannot start a range in a character class; escape the dash as \-"#),
        ),
        (
            r#"/[\p{L}-z]/"#,
            Err(r#"\p cannot start a range in a character class; escape the dash as \-"#),
        ),
        (
            "/[a[b]]/",
            Err(r#"nested character classes are not supported; escape [ as \["#),
        ),
        (
            "/[[a]]/",
            Err(r#"nested character classes are not supported; escape [ as \["#),
        ),
        (
            "/[a-z&&b]/",
            Err(r#"&& in a character class is not supported; escape it as \&\&"#),
        ),
        (
            "/[a--b]/",
            Err(r#"-- in a character class is not supported; escape it as \-\-"#),
        ),
        (
            "/[a~~b]/",
            Err(r#"~~ in a character class is not supported; escape it as \~\~"#),
        ),
        (
            r#"/\ba/"#,
            Ok("01001011000000001110000000110000000010010000010000000000000000"),
        ),
        (
            r#"/\x{1F600}/"#,
            Ok("00000000000000000000000000000000100000000000000000000000000000"),
        ),
        (
            "/a{0,3}/",
            Ok("11111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            r#"/\P{L}/"#,
            Ok("00000000000000011111111110000000111111111111111100000000011000"),
        ),
        (
            r#"/[\d\-z]/"#,
            Ok("00000000000000000000000000000000010010110001000000000000000000"),
        ),
        (
            "/[a-z]/",
            Ok("01011011101010001110000000110000000110010000110000000000000010"),
        ),
        (
            "/[[:alpha:]]/",
            Ok("01111111111011001110000000110000000110010001110000000000000011"),
        ),
        (
            "/[[:^digit:]x]/",
            Ok("01111111111111111111111111111111101111111111111111111111111111"),
        ),
        (
            r#"/[\[\]]/"#,
            Ok("00000000000000000000000000000000000000001000000000000000000000"),
        ),
        (
            "/[]a]/",
            Ok("01001011000000001110000000110000000110011000110000000000000010"),
        ),
        (
            "/[^]a]/",
            Ok("00111110111111111111111111111111111111110111111111111111111111"),
        ),
        (
            "/[a-]/",
            Ok("01001011000000001110000000110000000110110000110000000000000010"),
        ),
        (
            "/[a&b~c-]/",
            Ok("01011011000000001110000000110000000110110000110000000000000010"),
        ),
        (
            r#"/\{,3\}/"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/a{2}/",
            Ok("00000001000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/a{2,}/",
            Ok("00000001000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/a\{x\}/"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/[{]/",
            Ok("00000000000000000000000000000000000011000000000000000000000000"),
        ),
        ("/a/ii", Err("duplicate regex flag 'i'")),
        (
            "/a{x}/",
            Err(r#"{ must start a repetition such as {2} or {1,3}; escape a literal brace as \{"#),
        ),
        (
            "/a{1,2/",
            Err(r#"{ must start a repetition such as {2} or {1,3}; escape a literal brace as \{"#),
        ),
        (
            "/{/",
            Err(r#"{ must start a repetition such as {2} or {1,3}; escape a literal brace as \{"#),
        ),
        (
            r#"/\w/i"#,
            Ok("01111111111111101110000000110000010110010011110000000000000011"),
        ),
        (
            r#"/(?i)\w/"#,
            Ok("01111111111111101110000000110000010110010011110000000000000011"),
        ),
        (
            r#"/[\w]/i"#,
            Ok("01111111111111101110000000110000010110010011110000000000000011"),
        ),
        (
            r#"/\W/i"#,
            Ok("00000000000000011111111111111111101011111101111111111111111110"),
        ),
        (
            r#"/[^\w]/i"#,
            Ok("00000000000000011111111111111111101011111101111111111111111110"),
        ),
        (
            r#"/[\W]/i"#,
            Ok("00000000000000011111111111111111101011111101111111111111111110"),
        ),
        (
            r#"/[^\W]/i"#,
            Ok("01111111111111101110000000110000010110010011110000000000000011"),
        ),
        (
            "/k/i",
            Ok("00000000011100000000000000000000000000000000000000000000000000"),
        ),
        (
            "/K/i",
            Ok("00000000011100000000000000000000000000000000000000000000000000"),
        ),
        (
            "/s/i",
            Ok("00000000000011100000000000000000000000000000100000000000000011"),
        ),
        (
            r#"/\x{212A}/i"#,
            Ok("00000000011100000000000000000000000000000000000000000000000000"),
        ),
        (
            "/[a-z]/i",
            Ok("01111111111111101110000000110000000110010001110000000000000011"),
        ),
        (
            "/[a-z]+/i",
            Ok("01111111111111101110000000110000000110010001110000000000000011"),
        ),
        (
            "/[[:upper:]]/i",
            Ok("01111111111111101110000000110000000110010001110000000000000011"),
        ),
        (
            "/[[:^upper:]]/i",
            Ok("00000000000000011111111111111111111111111111111111111111111110"),
        ),
        (
            "/[[:word:]]/i",
            Ok("01111111111111101110000000110000010110010011110000000000000011"),
        ),
        (
            "/[[:lower:]]/",
            Ok("01011011101010001110000000110000000110010000110000000000000010"),
        ),
        (
            r#"/\pL/i"#,
            Ok("01111111111111101110000001111111000110010001110011111111100111"),
        ),
        (
            r#"/\p{Lu}/i"#,
            Ok("01111111111111101110000001111110000110010001110011111111000011"),
        ),
        (
            r#"/\p{Lu}/"#,
            Ok("00100100010101000000000000000100000000000001000001100010000001"),
        ),
        (
            r#"/\P{Lu}/i"#,
            Ok("00000000000000011111111110000001111111111111111100000000111100"),
        ),
        (
            r#"/[^\p{Lu}]/i"#,
            Ok("00000000000000011111111110000001111111111111111100000000111100"),
        ),
        (
            r#"/\p{Greek}/i"#,
            Ok("00000000000000000000000000001110000000000000000000110000000000"),
        ),
        (
            r#"/\p{Greek}/"#,
            Ok("00000000000000000000000000001110000000000000000000110000000000"),
        ),
        (
            r#"/\p{greek}/"#,
            Ok("00000000000000000000000000001110000000000000000000110000000000"),
        ),
        (
            r#"/\p{GREEK}/"#,
            Ok("00000000000000000000000000001110000000000000000000110000000000"),
        ),
        (
            r#"/\p{Old_Italic}/"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/\p{OldItalic}/"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/\p{old italic}/"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/\p{Letter}/"#,
            Ok("01111111111111101110000001111111000110010001110011111111100111"),
        ),
        (
            r#"/\p{Uppercase_Letter}/"#,
            Ok("00100100010101000000000000000100000000000001000001100010000001"),
        ),
        (
            r#"/\p{Uppercase Letter}/"#,
            Ok("00100100010101000000000000000100000000000001000001100010000001"),
        ),
        (
            r#"/\p{uppercase-letter}/"#,
            Ok("00100100010101000000000000000100000000000001000001100010000001"),
        ),
        (
            r#"/\p{L&}/"#,
            Err(r#"error parsing regexp: invalid character class range: `\p{L&}`"#),
        ),
        (
            r#"/\p{LC}/"#,
            Ok("01111111111111101110000001111110000110010001110011111111000011"),
        ),
        (
            r#"/\p{Lc}/"#,
            Ok("01111111111111101110000001111110000110010001110011111111000011"),
        ),
        (
            r#"/\p{Any}/"#,
            Ok("01111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            r#"/\p{Assigned}/"#,
            Ok("01111111111111111111111111111111111111111111111011111111101111"),
        ),
        (
            r#"/\P{Assigned}/"#,
            Ok("00000000000000000000000000000000000000000000000100000000010000"),
        ),
        (
            r#"/\p{ASCII}/"#,
            Ok("01111111111011011111111000110000010111111111111000000000000011"),
        ),
        (
            r#"/\p{Ascii}/i"#,
            Ok("01111111111111111111111000110000010111111111111000000000000011"),
        ),
        (
            r#"/\p{Cn}/"#,
            Ok("00000000000000000000000000000000000000000000000100000000010000"),
        ),
        (
            r#"/\pC/"#,
            Ok("00000000000000011111101000000000000000000000001100000000011000"),
        ),
        (
            r#"/\p{C}/"#,
            Ok("00000000000000011111101000000000000000000000001100000000011000"),
        ),
        (
            r#"/\p{Sc}/"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/\p{sc=Greek}/"#,
            Err(r#"error parsing regexp: invalid character class range: `\p{sc=Greek}`"#),
        ),
        (
            r#"/\p{gc=Lu}/"#,
            Err(r#"error parsing regexp: invalid character class range: `\p{gc=Lu}`"#),
        ),
        (
            r#"/\p{Script=Greek}/"#,
            Err(r#"error parsing regexp: invalid character class range: `\p{Script=Greek}`"#),
        ),
        (
            r#"/\p{Alphabetic}/"#,
            Err(r#"error parsing regexp: invalid character class range: `\p{Alphabetic}`"#),
        ),
        (
            r#"/\p{Emoji}/"#,
            Err(r#"error parsing regexp: invalid character class range: `\p{Emoji}`"#),
        ),
        (
            r#"/\p{Han}/"#,
            Ok("00000000000000000000000000000001000000000000000000000000000000"),
        ),
        (
            r#"/\pN/"#,
            Ok("00000000000000000000000000000000011010000001000000000000000000"),
        ),
        (
            r#"/\pl/"#,
            Ok("01111111111111101110000001111111000110010001110011111111100111"),
        ),
        (
            r#"/\pL/"#,
            Ok("01111111111111101110000001111111000110010001110011111111100111"),
        ),
        (
            r#"/\PL/"#,
            Ok("00000000000000011111111110000000111111111111111100000000011000"),
        ),
        (
            r#"/\p{IsGreek}/"#,
            Err(r#"error parsing regexp: invalid character class range: `\p{IsGreek}`"#),
        ),
        (
            r#"/\p{InGreek}/"#,
            Err(r#"error parsing regexp: invalid character class range: `\p{InGreek}`"#),
        ),
        (
            r#"/\p{Common}/"#,
            Ok("00000000000000011111111110000000110111111111111000000000000000"),
        ),
        (
            r#"/\p{Zyyy}/"#,
            Err(r#"error parsing regexp: invalid character class range: `\p{Zyyy}`"#),
        ),
        (
            r#"/\p{Latn}/"#,
            Err(r#"error parsing regexp: invalid character class range: `\p{Latn}`"#),
        ),
        (
            r#"/\p{Latin}/i"#,
            Ok("01111111111111101110000001110000000110010001110011001111000011"),
        ),
        (
            r#"/\p{Cherokee}/i"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/\p{Lt}/i"#,
            Ok("00000000000000000000000000000000000000000000000000001100000000"),
        ),
        (
            r#"/\p{Ll}/i"#,
            Ok("01111111111111101110000001111110000110010001110011111111000011"),
        ),
        (
            r#"/\p/"#,
            Err(r#"error parsing regexp: invalid character class range: `\p`"#),
        ),
        (
            r#"/\p{}/"#,
            Err(r#"error parsing regexp: invalid character class range: `\p{}`"#),
        ),
        (
            r#"/\p{L/"#,
            Err(r#"{ must start a repetition such as {2} or {1,3}; escape a literal brace as \{"#),
        ),
        (
            "/\\p\u{e9}/",
            Err("error parsing regexp: invalid character class range: `\\p\u{e9}`"),
        ),
        (
            r#"/\p^L/"#,
            Err(r#"error parsing regexp: invalid character class range: `\p^`"#),
        ),
        (
            r#"/\P/"#,
            Err(r#"error parsing regexp: invalid character class range: `\P`"#),
        ),
        (
            r#"/\s/"#,
            Ok("00000000000000011110111000000000000000000000000000000000000000"),
        ),
        (
            r#"/[\s]/"#,
            Ok("00000000000000011110111000000000000000000000000000000000000000"),
        ),
        (
            r#"/\S/"#,
            Ok("01111111111111101111000111111111111111111111111111111111111111"),
        ),
        (
            r#"/[\S]/"#,
            Ok("01111111111111101111000111111111111111111111111111111111111111"),
        ),
        (
            "/[[:space:]]/",
            Ok("00000000000000011111111000000000000000000000000000000000000000"),
        ),
        (
            "/[[:blank:]]/",
            Ok("00000000000000000000110000000000000000000000000000000000000000"),
        ),
        (
            r#"/\v/"#,
            Ok("00000000000000000001000000000000000000000000000000000000000000"),
        ),
        (
            r#"/[\v]/"#,
            Ok("00000000000000000001000000000000000000000000000000000000000000"),
        ),
        (
            r#"/\x0b/"#,
            Ok("00000000000000000001000000000000000000000000000000000000000000"),
        ),
        (
            "/\\b\u{e9}/",
            Ok("00000000000000000000000000100000000000000000000000000000000000"),
        ),
        (
            r#"/a\b/"#,
            Ok("01000001000000001110000000110000000010010000010000000000000010"),
        ),
        (
            "/\\B\u{e9}/",
            Ok("00000000000000000000000001010000000000000000000000000000000000"),
        ),
        (
            "/\u{e9}\\b/",
            Ok("00000000000000000000000000010000000000000000000000000000000000"),
        ),
        (
            r#"/\b/"#,
            Ok("01111111111011001110000000110000010110010011110000000000000011"),
        ),
        (
            r#"/\B/"#,
            Ok("10001111000100110011111111111111111111101101101111111111111111"),
        ),
        (
            r#"/a\B/"#,
            Ok("00001011000000000000000000000000000100000000100000000000000000"),
        ),
        (
            r#"/\bfoo\b/"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/a**/",
            Err("error parsing regexp: invalid nested repetition operator: `**`"),
        ),
        (
            "/a*?*/",
            Err("error parsing regexp: invalid nested repetition operator: `*?*`"),
        ),
        (
            "/a*??/",
            Err("error parsing regexp: invalid nested repetition operator: `*??`"),
        ),
        (
            "/a+*/",
            Err("error parsing regexp: invalid nested repetition operator: `+*`"),
        ),
        (
            "/a{2}{3}/",
            Err("error parsing regexp: invalid nested repetition operator: `{2}{3}`"),
        ),
        (
            "/a*{2}/",
            Err("error parsing regexp: invalid nested repetition operator: `*{2}`"),
        ),
        (
            "/a**?/",
            Err("error parsing regexp: invalid nested repetition operator: `**?`"),
        ),
        (
            "/(?:a*)*/",
            Ok("11111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/(a*)*/",
            Ok("11111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/a*(?i)*/",
            Ok("11111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/a(?i)*/",
            Ok("11111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/(?i)*/",
            Err("error parsing regexp: missing argument to repetition operator: `*`"),
        ),
        (
            "/*/",
            Err("error parsing regexp: missing argument to repetition operator: `*`"),
        ),
        (
            "/a|*/",
            Err("error parsing regexp: missing argument to repetition operator: `*`"),
        ),
        (
            "/(*)/",
            Err("error parsing regexp: missing argument to repetition operator: `*`"),
        ),
        (
            "/^*/",
            Ok("11111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/$*/",
            Ok("11111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            r#"/\b*/"#,
            Ok("11111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/(?:)*/",
            Ok("11111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/a{2}?/",
            Ok("00000001000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/a{2}?{3}/",
            Err("error parsing regexp: invalid nested repetition operator: `{2}?{3}`"),
        ),
        (
            "/a??/",
            Ok("11111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/a+?/",
            Ok("01001011000000001110000000110000000110010000110000000000000010"),
        ),
        (
            "/x{2,3}?/",
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/a{1000}/",
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/a{1001}/",
            Err("error parsing regexp: invalid repeat count: `{1001}`"),
        ),
        (
            "/a{0,1000}/",
            Ok("11111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/a{0,1001}/",
            Err("error parsing regexp: invalid repeat count: `{0,1001}`"),
        ),
        (
            "/a{1000,}/",
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/a{1001,}/",
            Err("error parsing regexp: invalid repeat count: `{1001,}`"),
        ),
        (
            "/(?:a{2}){500}/",
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/(?:a{2}){501}/",
            Err("error parsing regexp: invalid repeat count: `{501}`"),
        ),
        (
            "/(?:a{10}){100}/",
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/(?:a{10}){101}/",
            Err("error parsing regexp: invalid repeat count: `{101}`"),
        ),
        (
            "/((a{10}){10}){10}/",
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/((a{10}){10}){11}/",
            Err("error parsing regexp: invalid repeat count: `{11}`"),
        ),
        (
            "/(?:a{1000})*/",
            Ok("11111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/(?:a{1000}){1}/",
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/(?:a{1000}){0,1}/",
            Ok("11111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/(?:a{1000}){1,}/",
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/(?:(?:a{1000}){1,}){2}/",
            Err("error parsing regexp: invalid repeat count: `{2}`"),
        ),
        (
            "/(?:a{1000}){0}/",
            Ok("11111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/(?:a{2}|b{3}){400}/",
            Err("error parsing regexp: invalid repeat count: `{400}`"),
        ),
        (
            "/(?:a{2}|b{3}){300}/",
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/a{2,1}/",
            Err("error parsing regexp: invalid repeat count: `{2,1}`"),
        ),
        (
            "/a{01}/",
            Ok("00000000000000000000000000000000000010000000000000000000000000"),
        ),
        (
            "/a{0}/",
            Ok("11111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/a{1,01}/",
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/a{00}/",
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/a{99999999999}/",
            Err("error parsing regexp: invalid repeat count: `{99999999999}`"),
        ),
        (
            "/a{1,99999999999}/",
            Err("error parsing regexp: invalid repeat count: `{1,99999999999}`"),
        ),
        (
            "/a{100000000}/",
            Err("error parsing regexp: invalid repeat count: `{100000000}`"),
        ),
        (
            "/a{99999999}/",
            Err("error parsing regexp: invalid repeat count: `{99999999}`"),
        ),
        (
            "/{2}/",
            Err("error parsing regexp: missing argument to repetition operator: `{2}`"),
        ),
        (
            r#"/\pL{1000}/"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/(?:\pL{100}){10}/"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/[0-9]{1000}/",
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/(?i)a/",
            Ok("01101111000000001110000000110000000110010000110000000000000011"),
        ),
        (
            "/(?i:a)b/",
            Ok("00001010000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/(?-i)a/i",
            Ok("01001011000000001110000000110000000110010000110000000000000010"),
        ),
        (
            "/(?i-i)a/",
            Ok("01001011000000001110000000110000000110010000110000000000000010"),
        ),
        (
            "/(?ii)a/",
            Ok("01101111000000001110000000110000000110010000110000000000000011"),
        ),
        (
            "/(?)a/",
            Ok("01001011000000001110000000110000000110010000110000000000000010"),
        ),
        (
            "/(?-)a/",
            Err("error parsing regexp: invalid or unsupported Perl syntax: `(?-)`"),
        ),
        (
            "/(?i-)a/",
            Err("error parsing regexp: invalid or unsupported Perl syntax: `(?i-)`"),
        ),
        (
            "/(?--i)a/",
            Err("error parsing regexp: invalid or unsupported Perl syntax: `(?--`"),
        ),
        (
            "/(?i-s-m)a/",
            Err("error parsing regexp: invalid or unsupported Perl syntax: `(?i-s-`"),
        ),
        (
            "/(?x)a/",
            Err("error parsing regexp: invalid or unsupported Perl syntax: `(?x`"),
        ),
        (
            "/(?u)a/",
            Err("error parsing regexp: invalid or unsupported Perl syntax: `(?u`"),
        ),
        (
            "/(?R)a/",
            Err("error parsing regexp: invalid or unsupported Perl syntax: `(?R`"),
        ),
        (
            "/(?U)a+/",
            Ok("01001011000000001110000000110000000110010000110000000000000010"),
        ),
        (
            "/(?s)./",
            Ok("01111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/(?m)^b/",
            Ok("00010000000000001100000000000000000000000000000000000000000000"),
        ),
        (
            "/(?sm:^.)/",
            Ok("01111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/(?P<name>a)/",
            Ok("01001011000000001110000000110000000110010000110000000000000010"),
        ),
        (
            "/(?<name>a)/",
            Ok("01001011000000001110000000110000000110010000110000000000000010"),
        ),
        (
            "/(?P<1>a)/",
            Ok("01001011000000001110000000110000000110010000110000000000000010"),
        ),
        (
            "/(?<a>x)(?<a>y)/",
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/(?P<n-m>a)/",
            Err("error parsing regexp: invalid named capture: `(?P<n-m>`"),
        ),
        (
            "/(?P<>a)/",
            Err("error parsing regexp: invalid named capture: `(?P<>`"),
        ),
        (
            "/(?P=name)/",
            Err("error parsing regexp: invalid or unsupported Perl syntax: `(?P`"),
        ),
        (
            "/(?'n'a)/",
            Err("error parsing regexp: invalid or unsupported Perl syntax: `(?'`"),
        ),
        (
            "/(?#comment)/",
            Err("error parsing regexp: invalid or unsupported Perl syntax: `(?#`"),
        ),
        (
            "/(?=a)/",
            Err("error parsing regexp: invalid or unsupported Perl syntax: `(?=`"),
        ),
        (
            "/(?!a)/",
            Err("error parsing regexp: invalid or unsupported Perl syntax: `(?!`"),
        ),
        (
            "/(?<=a)b/",
            Err("error parsing regexp: invalid named capture: `(?<=a)b`"),
        ),
        (
            "/(?<!a)b/",
            Err("error parsing regexp: invalid named capture: `(?<!a)b`"),
        ),
        (
            "/(?>a)/",
            Err("error parsing regexp: invalid or unsupported Perl syntax: `(?>`"),
        ),
        (
            "/(?i/",
            Err("error parsing regexp: invalid or unsupported Perl syntax: `(?i`"),
        ),
        (
            "/(?/",
            Err("error parsing regexp: invalid or unsupported Perl syntax: `(?`"),
        ),
        (
            "/(?P<na/",
            Err("error parsing regexp: invalid named capture: `(?P<na`"),
        ),
        (
            "/(?P<\u{e9}>a)/",
            Err("error parsing regexp: invalid named capture: `(?P<\u{e9}>`"),
        ),
        (
            r#"/\Qa\E/"#,
            Err(r#"\Q...\E is not supported; escape each character instead"#),
        ),
        (
            r#"/\\Q/"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/\x41/"#,
            Ok("00100100000000000000000000000000000000000000000000000000000001"),
        ),
        (
            r#"/\x4/"#,
            Err(r#"error parsing regexp: invalid escape sequence: `\x4`"#),
        ),
        (
            r#"/\x{}/"#,
            Err(r#"error parsing regexp: invalid escape sequence: `\x{}`"#),
        ),
        (
            r#"/\x{110000}/"#,
            Err(r#"error parsing regexp: invalid escape sequence: `\x{110000`"#),
        ),
        (
            r#"/\x{10FFFF}/"#,
            Ok("00000000000000000000000000000000000000000000000100000000000000"),
        ),
        (
            r#"/\x{D800}/"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/\x{D800}?a/"#,
            Ok("01001011000000001110000000110000000110010000110000000000000010"),
        ),
        (
            r#"/[\x{D800}-\x{DFFF}]/"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/[\x{D7FF}-\x{E000}]/"#,
            Ok("00000000000000000000000000000000000000000000000000000000001000"),
        ),
        (
            r#"/\x{zz}/"#,
            Err(r#"error parsing regexp: invalid escape sequence: `\x{z`"#),
        ),
        (
            r#"/\x{41/"#,
            Err(r#"{ must start a repetition such as {2} or {1,3}; escape a literal brace as \{"#),
        ),
        (
            r#"/\xg1/"#,
            Err(r#"error parsing regexp: invalid escape sequence: `\xg1`"#),
        ),
        (
            r#"/[^\x{0}-\x{10FFFF}]/"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/\x{0000000041}/"#,
            Ok("00100100000000000000000000000000000000000000000000000000000001"),
        ),
        (
            r#"/\x/"#,
            Err(r#"error parsing regexp: invalid escape sequence: `\x`"#),
        ),
        (
            r#"/\u0041/"#,
            Err(r#"error parsing regexp: invalid escape sequence: `\u`"#),
        ),
        (
            r#"/\U00000041/"#,
            Err(r#"error parsing regexp: invalid escape sequence: `\U`"#),
        ),
        (
            r#"/\e/"#,
            Err(r#"error parsing regexp: invalid escape sequence: `\e`"#),
        ),
        (
            r#"/\a/"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/\f/"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/\t/"#,
            Ok("00000000000000000000100000000000000000000000000000000000000000"),
        ),
        (
            r#"/\n/"#,
            Ok("00000000000000011110001000000000000000000000000000000000000000"),
        ),
        (
            r#"/\r/"#,
            Ok("00000000000000000000001000000000000000000000000000000000000000"),
        ),
        (
            r#"/\_/"#,
            Ok("00000000000000000000000000000000000100000010000000000000000000"),
        ),
        (
            r#"/\-/"#,
            Ok("00000000000000000000000000000000000000110000000000000000000000"),
        ),
        (
            r#"/\ /"#,
            Ok("00000000000000000000010000000000000000000000000000000000000000"),
        ),
        (
            r#"/\#/"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/\~/"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/\@/"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/\%/"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/\'/"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/\"/"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/\\\u{e9}/",
            Err("error parsing regexp: invalid escape sequence: `\\\u{e9}`"),
        ),
        (
            r#"/\c/"#,
            Err(r#"error parsing regexp: invalid escape sequence: `\c`"#),
        ),
        (
            r#"/\k/"#,
            Err(r#"error parsing regexp: invalid escape sequence: `\k`"#),
        ),
        (
            r#"/\g/"#,
            Err(r#"error parsing regexp: invalid escape sequence: `\g`"#),
        ),
        (
            r#"/\h/"#,
            Err(r#"error parsing regexp: invalid escape sequence: `\h`"#),
        ),
        (
            r#"/\K/"#,
            Err(r#"error parsing regexp: invalid escape sequence: `\K`"#),
        ),
        (
            r#"/\G/"#,
            Err(r#"error parsing regexp: invalid escape sequence: `\G`"#),
        ),
        (
            r#"/\X/"#,
            Err(r#"error parsing regexp: invalid escape sequence: `\X`"#),
        ),
        (
            r#"/\R/"#,
            Err(r#"error parsing regexp: invalid escape sequence: `\R`"#),
        ),
        (
            r#"/\N/"#,
            Err(r#"error parsing regexp: invalid escape sequence: `\N`"#),
        ),
        (
            r#"/\o{101}/"#,
            Err(r#"error parsing regexp: invalid escape sequence: `\o`"#),
        ),
        (
            r#"/\Z/"#,
            Err(r#"error parsing regexp: invalid escape sequence: `\Z`"#),
        ),
        (
            r#"/\z/"#,
            Ok("11111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            r#"/\A/"#,
            Ok("11111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            r#"/\C/"#,
            Err(r#"error parsing regexp: invalid escape sequence: `\C`"#),
        ),
        (
            r#"/a\/"#,
            Err("error parsing regexp: trailing backslash at end of expression: ``"),
        ),
        (
            r#"/\012/"#,
            Err(r#"\0 is not supported; use \x{...} for character codes"#),
        ),
        (
            r#"/\8/"#,
            Err(r#"\8 is not supported; use \x{...} for character codes"#),
        ),
        (
            r#"/a\z/"#,
            Ok("01000001000000000000000000010000000000000000000000000000000000"),
        ),
        (
            "/a$/",
            Ok("01000001000000000000000000010000000000000000000000000000000000"),
        ),
        (
            "/a$/m",
            Ok("01000001000000001110000000010000000000000000000000000000000000"),
        ),
        (
            "/^a/m",
            Ok("01001011000000001110000000100000000010010000010000000000000000"),
        ),
        (
            r#"/\Aa/m"#,
            Ok("01001011000000001010000000100000000010010000010000000000000000"),
        ),
        (
            r#"/a\z/m"#,
            Ok("01000001000000000000000000010000000000000000000000000000000000"),
        ),
        (
            "/a|/",
            Ok("11111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/|a/",
            Ok("11111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/|/",
            Ok("11111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/a||b/",
            Ok("11111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/()/",
            Ok("11111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/(|)/",
            Ok("11111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/(?:)/",
            Ok("11111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "//",
            Ok("11111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/(a|b)|c/",
            Ok("01011011000000001110000000110000000110010000110000000000000010"),
        ),
        (
            "/a|b|c/",
            Ok("01011011000000001110000000110000000110010000110000000000000010"),
        ),
        (
            "/ab|ac/",
            Ok("00001010000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/[ab]|c/",
            Ok("01011011000000001110000000110000000110010000110000000000000010"),
        ),
        (
            r#"/.|\n/"#,
            Ok("01111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/a|./",
            Ok("01111111111111101111111111111111111111111111111111111111111111"),
        ),
        (
            "/(?s:.)|a/",
            Ok("01111111111111111111111111111111111111111111111111111111111111"),
        ),
        ("/(/", Err("error parsing regexp: missing closing ): `(`")),
        ("/)/", Err("error parsing regexp: unexpected ): `)`")),
        ("/(a))/", Err("error parsing regexp: unexpected ): `(a))`")),
        (
            "/((a)/",
            Err("error parsing regexp: missing closing ): `((a)`"),
        ),
        ("/a)/", Err("error parsing regexp: unexpected ): `a)`")),
        ("/[]/", Err("error parsing regexp: missing closing ]: `[]`")),
        (
            "/[^]/",
            Err("error parsing regexp: missing closing ]: `[^]`"),
        ),
        ("/[a/", Err("error parsing regexp: missing closing ]: `[a`")),
        (
            "/[a-/",
            Err("error parsing regexp: missing closing ]: `[a-`"),
        ),
        (
            "/[z-a]/",
            Err("error parsing regexp: invalid character class range: `z-a`"),
        ),
        (
            r#"/[\b]/"#,
            Err(r#"error parsing regexp: invalid escape sequence: `\b`"#),
        ),
        (
            r#"/[\A]/"#,
            Err(r#"error parsing regexp: invalid escape sequence: `\A`"#),
        ),
        (
            r#"/[\z]/"#,
            Err(r#"error parsing regexp: invalid escape sequence: `\z`"#),
        ),
        (
            r#"/[a-\d]/"#,
            Err(r#"error parsing regexp: invalid escape sequence: `\d`"#),
        ),
        (
            r#"/[\d-]/"#,
            Ok("00000000000000000000000000000000010010110001000000000000000000"),
        ),
        (
            "/[-a]/",
            Ok("01001011000000001110000000110000000110110000110000000000000010"),
        ),
        (
            r#"/[a\-z]/"#,
            Ok("01001011000000001110000000110000000110110000110000000000000010"),
        ),
        (
            "/[[:alpha:]-z]/",
            Ok("01111111111011001110000000110000000110110001110000000000000011"),
        ),
        (
            "/[[:foo:]]/",
            Err("error parsing regexp: invalid character class range: `[:foo:]`"),
        ),
        (
            "/[[:alpha:]/",
            Err("error parsing regexp: missing closing ]: `[[:alpha:]`"),
        ),
        (
            "/[[:alpha]]/",
            Err(r#"nested character classes are not supported; escape [ as \["#),
        ),
        (
            "/[[=a=]]/",
            Err(r#"nested character classes are not supported; escape [ as \["#),
        ),
        (
            "/[[.a.]]/",
            Err(r#"nested character classes are not supported; escape [ as \["#),
        ),
        (
            r#"/[\Qa\E]/"#,
            Err(r#"\Q...\E is not supported; escape each character instead"#),
        ),
        (
            r#"/[\p{Greek}\d]/"#,
            Ok("00000000000000000000000000001110010010000001000000110000000000"),
        ),
        (
            r#"/[^\p{Greek}\d]/"#,
            Ok("01111111111111111111111111110001101111111111111111001111111111"),
        ),
        (
            r#"/[\P{L}a]/i"#,
            Ok("01101111000000011111111110110000111111111111111100000000011011"),
        ),
        (
            r#"/[\D\S]/"#,
            Ok("01111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/[[:^word:][:digit:]]/",
            Ok("00000000000100111111111111111111111011111101111111111111111110"),
        ),
        (
            "/[\u{e9}-\u{fc}]/i",
            Ok("00000000000000000000000001110000000000000000000000000000000000"),
        ),
        (
            "/[k]/i",
            Ok("00000000011100000000000000000000000000000000000000000000000000"),
        ),
        (
            "/[K]/",
            Ok("00000000010000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/[Aa]/",
            Ok("01101111000000001110000000110000000110010000110000000000000011"),
        ),
        (
            "/[Aa]b/i",
            Ok("00001110000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/x[Kk]/",
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/[\u{17f}]/i",
            Ok("00000000000011100000000000000000000000000000100000000000000011"),
        ),
        (
            r#"/[^\n]/"#,
            Ok("01111111111111101111111111111111111111111111111111111111111111"),
        ),
        (
            "/[^a]/s",
            Ok("00111110111111111111111111111111111111111111111111111111111111"),
        ),
        (
            r#"/[\x{0}-\x{10FFFF}]/"#,
            Ok("01111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            r#"/[\x00-\x{10FFFF}]/i"#,
            Ok("01111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/./",
            Ok("01111111111111101111111111111111111111111111111111111111111111"),
        ),
        (
            "/./s",
            Ok("01111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/^.$/",
            Ok("01110000111111100001110111000110101001101110001111111111111100"),
        ),
        (
            "/(?s)^.$/",
            Ok("01110000111111110001110111000110101001101110001111111111111100"),
        ),
        (
            "/\u{df}/i",
            Ok("00000000000000000000000000000000000000000000000011000000000010"),
        ),
        (
            "/\u{3c3}/i",
            Ok("00000000000000000000000000000110000000000000000000000000000000"),
        ),
        (
            "/\u{b5}/i",
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/\u{1c5}/i",
            Ok("00000000000000000000000000000000000000000000000000001100000000"),
        ),
        (
            "/i/i",
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/\u{3c9}/i",
            Ok("00000000000000000000000000000000000000000000000000110000000000"),
        ),
        (
            "/\u{fb00}/i",
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/(?i)ab|AB/",
            Ok("00001110000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/\{/"#,
            Ok("00000000000000000000000000000000000011000000000000000000000000"),
        ),
        (
            "/a{1}{2}/",
            Err("error parsing regexp: invalid nested repetition operator: `{1}{2}`"),
        ),
        (
            "/(?i)a*/",
            Ok("11111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/a*?b/",
            Ok("00011010000000001100000000000000000100010000010000000000000000"),
        ),
        (
            "/(?U)a*?/",
            Ok("11111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/a/ims",
            Ok("01101111000000001110000000110000000110010000110000000000000011"),
        ),
        (
            "/a/mi",
            Ok("01101111000000001110000000110000000110010000110000000000000011"),
        ),
        (
            "/a/is",
            Ok("01101111000000001110000000110000000110010000110000000000000011"),
        ),
        ("/a/mm", Err("duplicate regex flag 'm'")),
        ("/a/ss", Err("duplicate regex flag 's'")),
        (
            "/a/x",
            Err("error parsing regexp: invalid or unsupported Perl syntax: `(?x`"),
        ),
        (
            "|a|i",
            Ok("01101111000000001110000000110000000110010000110000000000000011"),
        ),
        (
            "|a/b|",
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/a|b/",
            Ok("01011011000000001110000000110000000110010000110000000000000010"),
        ),
        (
            "|a/b|i",
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/a\/b/"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/\//"#,
            Ok("00000000000000000000000000000000000000000001100000000000000000"),
        ),
        (
            r#"/[\/]/"#,
            Ok("00000000000000000000000000000000000000000001100000000000000000"),
        ),
        (
            r#"/\pN+/"#,
            Ok("00000000000000000000000000000000011010000001000000000000000000"),
        ),
        (
            r#"/\p{Nd}/"#,
            Ok("00000000000000000000000000000000011010000001000000000000000000"),
        ),
        (
            r#"/\d/"#,
            Ok("00000000000000000000000000000000010010000001000000000000000000"),
        ),
        (
            r#"/\D/"#,
            Ok("01111111111111111111111111111111101111111111111111111111111111"),
        ),
        (
            r#"/[\d]/"#,
            Ok("00000000000000000000000000000000010010000001000000000000000000"),
        ),
        (
            r#"/[^\d]/"#,
            Ok("01111111111111111111111111111111101111111111111111111111111111"),
        ),
        (
            r#"/\w+/"#,
            Ok("01111111111011001110000000110000010110010011110000000000000011"),
        ),
        (
            "/[[:alpha:]]+/",
            Ok("01111111111011001110000000110000000110010001110000000000000011"),
        ),
        (
            "/[[:^alpha:]]/",
            Ok("00000000000100111111111111111111111111111111111111111111111110"),
        ),
        (
            "/[[:punct:]]/",
            Ok("00000000000000000000000000000000000111111111110000000000000000"),
        ),
        (
            "/[[:graph:]]/",
            Ok("01111111111011001110000000110000010111111111110000000000000011"),
        ),
        (
            "/[[:print:]]/",
            Ok("01111111111011001110010000110000010111111111110000000000000011"),
        ),
        (
            "/[[:cntrl:]]/",
            Ok("00000000000000011111101000000000000000000000001000000000000000"),
        ),
        (
            "/[[:xdigit:]]/",
            Ok("01111111000000001110000000110000010110010001110000000000000011"),
        ),
        (
            "/[[:ascii:]]/",
            Ok("01111111111011011111111000110000010111111111111000000000000011"),
        ),
        (
            "/[[:^ascii:]]/",
            Ok("00000000000100100000000111111111101000000000000111111111111110"),
        ),
        (
            "/[[:alnum:]]/i",
            Ok("01111111111111101110000000110000010110010001110000000000000011"),
        ),
        (
            "/[[:digit:]]/i",
            Ok("00000000000000000000000000000000010010000001000000000000000000"),
        ),
        (
            "/[[:^lower:]]/i",
            Ok("00000000000000011111111111111111111111111111111111111111111110"),
        ),
        (
            "/[^[:lower:]]/",
            Ok("00100100010101111111111111111111111111111111111111111111111111"),
        ),
        (
            r#"/\x{1E943}/i"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/\x{1E921}/i"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/(?i)[\x{1E900}-\x{1E921}]/"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/[^\x{1E900}-\x{1E921}]/i"#,
            Ok("01111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            r#"/(?i)\x{10400}/"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/(a)(b)(c)(d)/",
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/((((a))))/",
            Ok("01001011000000001110000000110000000110010000110000000000000010"),
        ),
        (
            "/(?:(?:(?:a)))/",
            Ok("01001011000000001110000000110000000110010000110000000000000010"),
        ),
        (
            "/(?i)(?-i:a)/",
            Ok("01001011000000001110000000110000000110010000110000000000000010"),
        ),
        (
            "/(?i:a|b)c/",
            Ok("00000010000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/a(?i)b|c/",
            Ok("00001010000000000000000000000000000000000001000000000000000000"),
        ),
        (
            "/(a(?i)b)c/",
            Ok("00000010000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/(?m:$)/",
            Ok("11111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            "/(?m)$^/",
            Ok("10000000000000010010001000000000000000000000000000000000000000"),
        ),
        (
            "/$a/",
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/^$/",
            Ok("10000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            "/(?s:a.b)/",
            Ok("00000000000000001100000000000000000000010000010000000000000000"),
        ),
        (
            r#"/\p{Cs}/"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/\P{Cs}/"#,
            Ok("01111111111111111111111111111111111111111111111111111111111111"),
        ),
        (
            r#"/[\p{Cs}a]/"#,
            Ok("01001011000000001110000000110000000110010000110000000000000010"),
        ),
        (
            r#"/\p{Sidetic}/"#,
            Ok("00000000000000000000000000000000000000000000000000000000000100"),
        ),
        (
            r#"/\p{Tai_Yo}/"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/\p{TaiYo}/"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/\p{Beria Erfe}/"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/\p{Surrogate}/"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/\p{Ll}/"#,
            Ok("01011011101010101110000001111010000110010000110010010101000010"),
        ),
        (
            r#"/\x{A7CE}/i"#,
            Ok("00000000000000000000000000000000000000000000000000000011000000"),
        ),
        (
            r#"/\x{295}/i"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/[\x{A7CE}]/i"#,
            Ok("00000000000000000000000000000000000000000000000000000011000000"),
        ),
        (
            r#"/\p{Lo}/"#,
            Ok("00000000000000000000000000000001000000000000000000000000100100"),
        ),
        (
            r#"/\p{Mn}/i"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/\P{Ll}/i"#,
            Ok("00000000000000011111111110000001111111111111111100000000111100"),
        ),
        (
            r#"/[^\P{Ll}]/i"#,
            Ok("01111111111111101110000001111110000110010001110011111111000011"),
        ),
        (
            r#"/\p{Latin}/"#,
            Ok("01111111111111101110000001110000000110010001110011001111000011"),
        ),
        (
            r#"/\x{1E9E}/i"#,
            Ok("00000000000000000000000000000000000000000000000011000000000010"),
        ),
        (
            r#"/[\x{100}-\x{17F}]/i"#,
            Ok("00000000000011100000000000000000000000000000100000000000000011"),
        ),
        (
            r#"/[^\x{100}-\x{17F}]/i"#,
            Ok("01111111111100011111111111111111111111111111111111111111111111"),
        ),
        (
            "/(?i)[^k]/",
            Ok("01111111100011111111111111111111111111111111111111111111111111"),
        ),
        (
            r#"/(?i)[^\x{212A}]/"#,
            Ok("01111111100011111111111111111111111111111111111111111111111111"),
        ),
        (
            "/[[:^space:]]/",
            Ok("01111111111111101110000111111111111111111111111111111111111111"),
        ),
        (
            r#"/\p{Zs}/"#,
            Ok("00000000000000000000010100000000000000000000000000000000000000"),
        ),
        (
            r#"/\p{Z}/"#,
            Ok("00000000000000000000010110000000000000000000000000000000000000"),
        ),
        (
            r#"/\p{Zl}/"#,
            Ok("00000000000000000000000010000000000000000000000000000000000000"),
        ),
        (
            r#"/\p{Cc}/"#,
            Ok("00000000000000011111101000000000000000000000001000000000000000"),
        ),
        (
            r#"/\p{Cf}/"#,
            Ok("00000000000000000000000000000000000000000000000000000000000000"),
        ),
        (
            r#"/\p{Co}/"#,
            Ok("00000000000000000000000000000000000000000000000000000000001000"),
        ),
        (
            r#"/\p{L}+\p{N}*/"#,
            Ok("01111111111111101110000001111111000110010001110011111111100111"),
        ),
        (
            "/(?i)stra\u{df}e/",
            Ok("00000000000000000000000000000000000000000000000000000000000010"),
        ),
        (
            "/(?i)STRASSE/",
            Ok("00000000000000000000000000000000000000000000000000000000000001"),
        ),
    ];
}
