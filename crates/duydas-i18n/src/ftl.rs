//! Parser for the Fluent (FTL) syntax Duydaş uses, following the Fluent 1.0 grammar.
//!
//! Supported: messages and terms with attributes, comments, multiline patterns (with Fluent's
//! indentation rules), string and number literals, variables, message and term references, and
//! select expressions. Not supported, and reported as errors: functions (`NUMBER()`) and
//! parameterized terms. Std-only, because `build.rs` compiles this file too.

use std::fmt;

/// Placeables nested deeper than this are rejected, which bounds recursion in the parser, the
/// resolver and when the syntax tree is dropped.
const MAX_NESTING: usize = 16;

/// A parsed FTL file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Resource {
    /// Messages and terms in file order.
    pub entries: Vec<Entry>,
}

/// A top-level entry.
#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    /// A message (`id = ...`).
    Message(Message),
    /// A term (`-id = ...`); the id is stored without the dash.
    Term(Message),
}

/// A message or term.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    /// Identifier, without the leading dash for terms.
    pub id: String,
    /// The value; messages may consist of attributes only.
    pub value: Option<Pattern>,
    /// Attributes (`.name = ...`).
    pub attributes: Vec<Attribute>,
    /// 1-based line of the identifier.
    pub line: usize,
}

/// An attribute (`.name = pattern`).
#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    /// Attribute name.
    pub id: String,
    /// Attribute value.
    pub value: Pattern,
}

/// Text and placeables.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Pattern {
    /// Elements in order; adjacent text is merged.
    pub elements: Vec<Element>,
}

/// One piece of a pattern.
#[derive(Debug, Clone, PartialEq)]
pub enum Element {
    /// Literal text.
    Text(String),
    /// A placeable (`{ ... }`).
    Placeable(Expression),
}

/// The content of a placeable.
#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    /// A single value.
    Inline(Inline),
    /// A select expression (`{ selector -> [key] ... *[other] ... }`).
    Select {
        /// What is matched against the variant keys.
        selector: Inline,
        /// The variants; exactly one is the default.
        variants: Vec<Variant>,
    },
}

/// An inline expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    /// A string literal, with escapes resolved.
    String(String),
    /// A number literal as written, e.g. `-1.5`.
    Number(String),
    /// A variable (`$name`).
    Variable(String),
    /// A message reference (`message` or `message.attribute`).
    Message {
        /// Referenced message.
        id: String,
        /// Referenced attribute, if any.
        attribute: Option<String>,
    },
    /// A term reference (`-term` or `-term.attribute`).
    Term {
        /// Referenced term, without the dash.
        id: String,
        /// Referenced attribute, if any.
        attribute: Option<String>,
    },
    /// A nested placeable.
    Placeable(Box<Expression>),
}

/// One branch of a select expression.
#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    /// The key in brackets.
    pub key: VariantKey,
    /// The branch's text.
    pub value: Pattern,
    /// Whether this is the default (`*[...]`) branch.
    pub default: bool,
}

/// A variant key.
#[derive(Debug, Clone, PartialEq)]
pub enum VariantKey {
    /// A plural category or other identifier, e.g. `one`.
    Identifier(String),
    /// A number literal as written, e.g. `0`.
    Number(String),
}

/// A syntax error with its position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// 1-based line.
    pub line: usize,
    /// 1-based column, in characters.
    pub column: usize,
    /// What went wrong.
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.column, self.message)
    }
}

/// Parses an FTL file. Entries with errors are skipped (Fluent calls them junk); every error is
/// reported, so a caller can treat any error as fatal.
pub fn parse(source: &str) -> (Resource, Vec<ParseError>) {
    let normalized = source.replace("\r\n", "\n");
    let mut parser = Parser {
        src: &normalized,
        pos: 0,
        depth: 0,
        line_starts: std::iter::once(0)
            .chain(normalized.match_indices('\n').map(|(i, _)| i + 1))
            .collect(),
    };
    let mut resource = Resource::default();
    let mut errors = Vec::new();
    while let Some(c) = parser.peek() {
        let start = parser.pos;
        let result = match c {
            '\n' => {
                parser.bump();
                continue;
            }
            ' ' if parser.rest_of_line_is_blank() => {
                parser.skip_line();
                continue;
            }
            '#' => parser.comment().map(|()| None),
            '-' => parser.entry(true).map(Some),
            c if c.is_ascii_alphabetic() => parser.entry(false).map(Some),
            _ => Err(parser.error("expected a message, a term or a comment")),
        };
        match result {
            Ok(Some(entry)) => resource.entries.push(entry),
            Ok(None) => {}
            Err(error) => {
                errors.push(error);
                // Like Fluent's junk: drop from the start of the broken entry to the next line
                // that can start an entry, even if the error was found further down.
                parser.pos = start;
                parser.recover();
            }
        }
    }
    (resource, errors)
}

/// A pattern piece before indentation is resolved.
enum Raw {
    Text(String),
    /// Line ends (including blank lines) followed by an indented line.
    Block {
        newlines: usize,
        indent: usize,
    },
    Placeable(Expression),
}

struct Parser<'s> {
    src: &'s str,
    pos: usize,
    /// Current placeable nesting.
    depth: usize,
    /// Byte offset of each line's first character, for O(log n) line lookups.
    line_starts: Vec<usize>,
}

type PResult<T> = Result<T, ParseError>;

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.src[self.pos..].chars().nth(offset)
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn eat(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.pos += expected.len_utf8();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: char) -> PResult<()> {
        if self.eat(expected) {
            Ok(())
        } else {
            Err(self.error(&format!("expected \"{expected}\"")))
        }
    }

    fn error(&self, message: &str) -> ParseError {
        let line = self.line();
        let line_start = self.line_starts[line - 1];
        ParseError {
            line,
            column: self.src[line_start..self.pos].chars().count() + 1,
            message: message.to_owned(),
        }
    }

    /// 1-based line of the current position.
    fn line(&self) -> usize {
        self.line_starts.partition_point(|&start| start <= self.pos)
    }

    fn rest_of_line_is_blank(&self) -> bool {
        self.src[self.pos..]
            .split('\n')
            .next()
            .is_some_and(|line| line.chars().all(|c| c == ' '))
    }

    fn skip_line(&mut self) {
        match self.src[self.pos..].find('\n') {
            Some(i) => self.pos += i + 1,
            None => self.pos = self.src.len(),
        }
    }

    /// Skips to the next line that can start an entry.
    fn recover(&mut self) {
        self.skip_line();
        while let Some(c) = self.peek() {
            if c.is_ascii_alphabetic() || c == '-' || c == '#' {
                break;
            }
            self.skip_line();
        }
    }

    fn skip_blank_inline(&mut self) -> usize {
        let start = self.pos;
        while self.eat(' ') {}
        self.pos - start
    }

    /// Skips spaces and line ends; returns whether a line end was crossed.
    fn skip_blank(&mut self) -> bool {
        let mut crossed = false;
        while let Some(c) = self.peek() {
            match c {
                ' ' => {}
                '\n' => crossed = true,
                _ => break,
            }
            self.bump();
        }
        crossed
    }

    fn comment(&mut self) -> PResult<()> {
        let mut level = 0;
        while self.eat('#') {
            level += 1;
        }
        if level > 3 || !matches!(self.peek(), None | Some(' ' | '\n')) {
            return Err(self.error("comments start with #, ## or ### followed by a space"));
        }
        self.skip_line();
        Ok(())
    }

    fn identifier(&mut self) -> PResult<String> {
        let start = self.pos;
        match self.peek() {
            Some(c) if c.is_ascii_alphabetic() => {
                self.bump();
            }
            _ => return Err(self.error("expected an identifier")),
        }
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                self.bump();
            } else {
                break;
            }
        }
        Ok(self.src[start..self.pos].to_owned())
    }

    fn entry(&mut self, term: bool) -> PResult<Entry> {
        let line = self.line();
        if term {
            self.expect('-')?;
        }
        let id = self.identifier()?;
        self.skip_blank_inline();
        self.expect('=')?;
        self.skip_blank_inline();
        let value = self.pattern()?;
        let attributes = self.attributes()?;
        if !matches!(self.peek(), None | Some('\n')) {
            return Err(self.error("unexpected character after the entry"));
        }
        let message = Message {
            id,
            value,
            attributes,
            line,
        };
        if term {
            if message.value.is_none() {
                return Err(self.error("a term needs a value"));
            }
            Ok(Entry::Term(message))
        } else {
            if message.value.is_none() && message.attributes.is_empty() {
                return Err(self.error("a message needs a value or attributes"));
            }
            Ok(Entry::Message(message))
        }
    }

    fn attributes(&mut self) -> PResult<Vec<Attribute>> {
        let mut attributes = Vec::new();
        loop {
            let save = self.pos;
            if !self.skip_blank() || self.peek() != Some('.') {
                self.pos = save;
                return Ok(attributes);
            }
            self.bump();
            let id = self.identifier()?;
            self.skip_blank_inline();
            self.expect('=')?;
            self.skip_blank_inline();
            let value = self
                .pattern()?
                .ok_or_else(|| self.error("an attribute needs a value"))?;
            attributes.push(Attribute { id, value });
        }
    }

    fn pattern(&mut self) -> PResult<Option<Pattern>> {
        let mut raw = Vec::new();
        while let Some(c) = self.peek() {
            match c {
                '{' => raw.push(Raw::Placeable(self.placeable()?)),
                '}' => return Err(self.error("unbalanced closing brace in text")),
                '\n' => match self.block_start() {
                    Some(block) => raw.push(block),
                    None => break,
                },
                _ => {
                    let start = self.pos;
                    while let Some(c) = self.peek() {
                        if matches!(c, '{' | '}' | '\n') {
                            break;
                        }
                        self.bump();
                    }
                    raw.push(Raw::Text(self.src[start..self.pos].to_owned()));
                }
            }
        }
        Ok(finish_pattern(raw))
    }

    /// At a line end: consumes line ends and blank lines if the next non-blank line continues the
    /// pattern: an indented line not starting with [, *, . or a closing brace, or any line starting
    /// with a placeable.
    fn block_start(&mut self) -> Option<Raw> {
        let save = self.pos;
        let mut newlines = 0;
        while self.eat('\n') {
            newlines += 1;
            let indent = self.skip_blank_inline();
            match self.peek() {
                Some('\n') => continue,
                // A placeable may continue a pattern even without indentation.
                Some('{') => return Some(Raw::Block { newlines, indent }),
                Some(c) if indent > 0 && !matches!(c, '[' | '*' | '.' | '}') => {
                    return Some(Raw::Block { newlines, indent });
                }
                _ => break,
            }
        }
        self.pos = save;
        None
    }

    fn placeable(&mut self) -> PResult<Expression> {
        if self.depth >= MAX_NESTING {
            return Err(self.error("placeables are nested too deeply"));
        }
        self.depth += 1;
        let result = self.placeable_inner();
        self.depth -= 1;
        result
    }

    fn placeable_inner(&mut self) -> PResult<Expression> {
        self.expect('{')?;
        self.skip_blank();
        let inline = self.inline()?;
        self.skip_blank();
        let expression = if self.src[self.pos..].starts_with("->") {
            self.pos += 2;
            self.skip_blank_inline();
            match &inline {
                Inline::Message { .. } => {
                    return Err(self.error("a message reference cannot be a selector"));
                }
                Inline::Term {
                    attribute: None, ..
                } => return Err(self.error("a term value cannot be a selector")),
                Inline::Placeable(_) => {
                    return Err(self.error("a placeable cannot be a selector"));
                }
                _ => {}
            }
            let variants = self.variants()?;
            if !self.skip_blank() {
                return Err(
                    self.error("the closing brace of a select expression needs its own line")
                );
            }
            Expression::Select {
                selector: inline,
                variants,
            }
        } else if let Inline::Term {
            attribute: Some(_), ..
        } = inline
        {
            return Err(self.error("term attributes can only be used as selectors"));
        } else {
            Expression::Inline(inline)
        };
        self.expect('}')?;
        Ok(expression)
    }

    fn variants(&mut self) -> PResult<Vec<Variant>> {
        let mut variants = Vec::new();
        loop {
            let save = self.pos;
            let crossed = self.skip_blank();
            let default = self.peek() == Some('*');
            let key_start = if default {
                self.peek_at(1)
            } else {
                self.peek()
            };
            if key_start != Some('[') {
                self.pos = save;
                break;
            }
            if !crossed {
                return Err(self.error("each variant starts on a new line"));
            }
            if default {
                self.bump();
            }
            self.bump();
            self.skip_blank();
            let key = match self.peek() {
                Some(c) if c.is_ascii_digit() || c == '-' => VariantKey::Number(self.number()?),
                _ => VariantKey::Identifier(self.identifier()?),
            };
            self.skip_blank();
            self.expect(']')?;
            self.skip_blank_inline();
            let value = self
                .pattern()?
                .ok_or_else(|| self.error("a variant needs a value"))?;
            variants.push(Variant {
                key,
                value,
                default,
            });
        }
        match variants.iter().filter(|v| v.default).count() {
            1 => Ok(variants),
            0 => Err(self.error("a select expression needs exactly one *[default] variant")),
            _ => Err(self.error("a select expression has more than one default variant")),
        }
    }

    fn inline(&mut self) -> PResult<Inline> {
        match self.peek() {
            Some('"') => self.string().map(Inline::String),
            Some('$') => {
                self.bump();
                self.identifier().map(Inline::Variable)
            }
            Some('{') => Ok(Inline::Placeable(Box::new(self.placeable()?))),
            Some('-') if self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) => {
                self.number().map(Inline::Number)
            }
            Some('-') => {
                self.bump();
                let id = self.identifier()?;
                let attribute = self.attribute_accessor()?;
                if self.peek() == Some('(') {
                    return Err(self.error("parameterized terms are not supported"));
                }
                Ok(Inline::Term { id, attribute })
            }
            Some(c) if c.is_ascii_digit() => self.number().map(Inline::Number),
            Some(c) if c.is_ascii_alphabetic() => {
                let id = self.identifier()?;
                if self.peek() == Some('(') {
                    return Err(self.error("functions are not supported; format values in code"));
                }
                let attribute = self.attribute_accessor()?;
                Ok(Inline::Message { id, attribute })
            }
            _ => Err(self.error("expected a string, number, variable or reference")),
        }
    }

    fn attribute_accessor(&mut self) -> PResult<Option<String>> {
        if self.eat('.') {
            self.identifier().map(Some)
        } else {
            Ok(None)
        }
    }

    fn number(&mut self) -> PResult<String> {
        let start = self.pos;
        self.eat('-');
        let digits = |p: &mut Self| {
            let begin = p.pos;
            while p.peek().is_some_and(|c| c.is_ascii_digit()) {
                p.bump();
            }
            p.pos > begin
        };
        if !digits(self) {
            return Err(self.error("expected a number"));
        }
        if self.peek() == Some('.') && self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) {
            self.bump();
            digits(self);
        }
        Ok(self.src[start..self.pos].to_owned())
    }

    fn string(&mut self) -> PResult<String> {
        self.expect('"')?;
        let mut out = String::new();
        loop {
            match self.bump() {
                None | Some('\n') => return Err(self.error("unterminated string literal")),
                Some('"') => return Ok(out),
                Some('\\') => match self.bump() {
                    Some('\\') => out.push('\\'),
                    Some('"') => out.push('"'),
                    Some(kind @ ('u' | 'U')) => {
                        let len = if kind == 'u' { 4 } else { 6 };
                        let mut value = 0_u32;
                        for _ in 0..len {
                            // to_digit(16) accepts ASCII hex digits only, never a sign.
                            let Some(digit) = self.peek().and_then(|c| c.to_digit(16)) else {
                                return Err(
                                    self.error("a unicode escape needs exactly 4 or 6 hex digits")
                                );
                            };
                            self.bump();
                            value = value * 16 + digit;
                        }
                        // Surrogates and values above U+10FFFF become U+FFFD, as in fluent-syntax.
                        out.push(char::from_u32(value).unwrap_or(char::REPLACEMENT_CHARACTER));
                    }
                    _ => return Err(self.error("unknown escape sequence")),
                },
                Some(c) => out.push(c),
            }
        }
    }
}

/// Applies Fluent's indentation rules: strips the common indent of block lines, drops the line
/// ends before a pattern that starts on the next line, and trims trailing whitespace.
fn finish_pattern(raw: Vec<Raw>) -> Option<Pattern> {
    let common = raw
        .iter()
        .filter_map(|r| match r {
            Raw::Block { indent, .. } => Some(*indent),
            _ => None,
        })
        .min()
        .unwrap_or(0);
    let mut elements: Vec<Element> = Vec::new();
    let push_text = |elements: &mut Vec<Element>, text: &str| {
        if text.is_empty() {
            return;
        }
        if let Some(Element::Text(last)) = elements.last_mut() {
            last.push_str(text);
        } else {
            elements.push(Element::Text(text.to_owned()));
        }
    };
    for (i, piece) in raw.into_iter().enumerate() {
        match piece {
            Raw::Text(text) => push_text(&mut elements, &text),
            Raw::Block { newlines, indent } => {
                let mut text = if i == 0 {
                    String::new()
                } else {
                    "\n".repeat(newlines)
                };
                text.push_str(&" ".repeat(indent - common));
                push_text(&mut elements, &text);
            }
            Raw::Placeable(expression) => elements.push(Element::Placeable(expression)),
        }
    }
    if let Some(Element::Text(last)) = elements.last_mut() {
        let trimmed = last.trim_end_matches([' ', '\n']).len();
        last.truncate(trimmed);
        if last.is_empty() {
            elements.pop();
        }
    }
    (!elements.is_empty()).then_some(Pattern { elements })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(source: &str) -> Message {
        let (resource, errors) = parse(source);
        assert!(errors.is_empty(), "{errors:?}");
        match resource.entries.into_iter().next() {
            Some(Entry::Message(m) | Entry::Term(m)) => m,
            None => panic!("no entry in {source:?}"),
        }
    }

    fn text(p: &Pattern) -> String {
        p.elements
            .iter()
            .map(|e| match e {
                Element::Text(t) => t.clone(),
                Element::Placeable(_) => "{}".into(),
            })
            .collect()
    }

    #[test]
    fn simple_message_and_placeable() {
        let m = one("hello = Hello, { $name }!\n");
        assert_eq!(m.id, "hello");
        let value = m.value.unwrap_or_default();
        assert_eq!(text(&value), "Hello, {}!");
        assert_eq!(
            value.elements[1],
            Element::Placeable(Expression::Inline(Inline::Variable("name".into())))
        );
    }

    #[test]
    fn multiline_patterns_follow_fluent_indentation() {
        let m = one("multi =\n    First line\n      indented\n\n    after blank\n");
        assert_eq!(
            text(&m.value.unwrap_or_default()),
            "First line\n  indented\n\nafter blank"
        );
        let m = one("inline = Starts here\n    and continues\n");
        assert_eq!(
            text(&m.value.unwrap_or_default()),
            "Starts here\nand continues"
        );
    }

    #[test]
    fn select_expressions() {
        let m = one(
            "count =\n    { $count ->\n        [0] none\n        [one] { $n } item\n       *[other] { $n } items\n    }\n",
        );
        let Some(Element::Placeable(Expression::Select { selector, variants })) =
            m.value.unwrap_or_default().elements.into_iter().next()
        else {
            panic!("expected a select expression");
        };
        assert_eq!(selector, Inline::Variable("count".into()));
        assert_eq!(variants.len(), 3);
        assert_eq!(variants[0].key, VariantKey::Number("0".into()));
        assert!(variants[2].default);
        assert_eq!(text(&variants[1].value), "{} item");
    }

    #[test]
    fn attributes_terms_comments_and_references() {
        let (resource, errors) = parse(
            "### Section\n# Comment\n-brand = Duydaş\n    .gender = neuter\nwelcome = Hi { -brand } { other.title }\n    .title = Tooltip\nonly-attrs =\n    .label = Label\n",
        );
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(resource.entries.len(), 3);
        let Entry::Term(term) = &resource.entries[0] else {
            panic!("term")
        };
        assert_eq!(term.attributes[0].id, "gender");
        let Entry::Message(welcome) = &resource.entries[1] else {
            panic!("message")
        };
        assert_eq!(welcome.attributes[0].id, "title");
        let Entry::Message(attrs) = &resource.entries[2] else {
            panic!("message")
        };
        assert!(attrs.value.is_none());
    }

    #[test]
    fn string_literals_and_escapes() {
        let m = one(r#"braces = { "{" }{ "\u00e7\"" }"#);
        let elements = m.value.unwrap_or_default().elements;
        assert_eq!(
            elements[0],
            Element::Placeable(Expression::Inline(Inline::String("{".into())))
        );
        assert_eq!(
            elements[1],
            Element::Placeable(Expression::Inline(Inline::String("ç\"".into())))
        );
    }

    #[test]
    fn crlf_is_accepted() {
        let m = one("a = one\r\n    two\r\n");
        assert_eq!(text(&m.value.unwrap_or_default()), "one\ntwo");
    }

    #[test]
    fn errors_are_reported_and_recovered_from() {
        let (resource, errors) = parse(
            "bad = { $x ->\n    [one] x\n}\ngood = fine\nfn = { NUMBER($n) }\nbrace = a } b\n#nospace\n",
        );
        assert_eq!(errors.len(), 4, "{errors:?}");
        assert!(errors[0].message.contains("default"));
        assert!(errors[1].message.contains("functions"));
        assert!(errors[2].message.contains("closing brace"));
        assert!(errors[3].message.contains("comments"));
        assert_eq!(resource.entries.len(), 1);
        assert_eq!(errors[1].line, 5);
    }

    #[test]
    fn flush_left_placeables_and_attributes() {
        let m = one("m =\n{ $name }\n    text\n");
        assert_eq!(text(&m.value.unwrap_or_default()), "{}\n    text");
        let m = one("m = Hello\n\n.label = Tooltip\n");
        assert_eq!(m.attributes[0].id, "label");
    }

    #[test]
    fn recovery_keeps_the_next_entry() {
        let (resource, errors) =
            parse("bad = { \"unterminated\ngood = OK\nopen = { $x\nalso = fine\n");
        assert_eq!(errors.len(), 2, "{errors:?}");
        let ids: Vec<_> = resource
            .entries
            .iter()
            .map(|e| match e {
                Entry::Message(m) | Entry::Term(m) => m.id.as_str(),
            })
            .collect();
        assert_eq!(ids, ["good", "also"]);
    }

    #[test]
    fn deep_nesting_is_an_error_not_a_crash() {
        let source = format!("m = {}\"x\"{}\n", "{ ".repeat(10_000), " }".repeat(10_000));
        let (_, errors) = parse(&source);
        assert!(errors[0].message.contains("nested"), "{errors:?}");
        assert!(
            parse(&format!("m = {}1{}\n", "{".repeat(16), "}".repeat(16)))
                .1
                .is_empty()
        );
    }

    #[test]
    fn unicode_escapes_are_strict() {
        let (_, errors) = parse("m = { \"\\u+041\" }\n");
        assert!(errors[0].message.contains("hex"), "{errors:?}");
        let m = one("m = { \"\\uD800\" }\n");
        assert_eq!(
            m.value.unwrap_or_default().elements[0],
            Element::Placeable(Expression::Inline(Inline::String("\u{fffd}".into())))
        );
    }

    #[test]
    fn line_numbers_are_exact() {
        let (resource, errors) = parse("a = 1\n\n# c\nb = 2\nc = }\n");
        let Some(Entry::Message(b)) = resource.entries.get(1) else {
            panic!("b")
        };
        assert_eq!(b.line, 4);
        assert_eq!((errors[0].line, errors[0].column), (5, 5));
    }

    #[test]
    fn rejects_bad_selectors_and_empty_entries() {
        for source in [
            "a = { b ->\n *[x] y\n}\n",
            "a = { -t ->\n *[x] y\n}\n",
            "a =\n",
            "-t =\n    .attr = x\n",
            "a = { $x ->\n *[a] a\n *[b] b\n}\n",
            "a = { \"unterminated }\n",
            "a = { $x -> *[x] y\n}\n",
            "-t = T\n    .a = x\nm = { -t.a }\n",
        ] {
            let (_, errors) = parse(source);
            assert!(!errors.is_empty(), "accepted {source:?}");
        }
    }
}
