//! Resolves parsed FTL messages for one locale: variables, references, plural selection.
//!
//! Resources are checked with `schema::validate` before they reach a bundle, so unknown
//! references and reference cycles are already excluded; the resolver still fails safely on them.

use std::borrow::Cow;
use std::collections::HashMap;
use std::ptr;

use crate::format::{Lang, decimal_eq, normalize_literal};
use crate::ftl::{self, Element, Entry, Expression, Inline, Pattern, Variant, VariantKey};
use crate::{Args, Value};

/// Rewrites pattern text (pseudo-localization).
pub(crate) type Transform = for<'a> fn(&'a str) -> Cow<'a, str>;

/// Placeables resolved for one message, references included. Bounds the work and output of a
/// message that references another many times over (the same limit fluent-rs uses).
const MAX_PLACEABLES: usize = 100;

/// Messages and terms for one locale.
#[derive(Debug)]
pub(crate) struct Bundle {
    lang: Lang,
    messages: HashMap<String, ftl::Message>,
    terms: HashMap<String, ftl::Message>,
    /// Applied to pattern text only, never to interpolated values.
    transform: Option<Transform>,
}

/// State for formatting one message.
struct Scope<'b, 'a> {
    args: Option<&'a Args<'a>>,
    /// Inside a term: the caller's variables are invisible, and a select on one takes the
    /// default variant (Fluent semantics).
    in_term: bool,
    transform: Option<Transform>,
    placeables: usize,
    /// Patterns being resolved, to stop reference cycles.
    active: Vec<&'b Pattern>,
}

/// A resolved selector value.
enum Selector<'s> {
    Int(i64),
    Literal(&'s str),
    Str(String),
}

impl Bundle {
    pub(crate) fn new(lang: Lang, transform: Option<Transform>) -> Self {
        Self {
            lang,
            messages: HashMap::new(),
            terms: HashMap::new(),
            transform,
        }
    }

    /// Adds validated entries (validation rejects duplicate ids).
    pub(crate) fn add(&mut self, resource: ftl::Resource) {
        for entry in resource.entries {
            match entry {
                Entry::Message(m) => self.messages.insert(m.id.clone(), m),
                Entry::Term(t) => self.terms.insert(t.id.clone(), t),
            };
        }
    }

    pub(crate) fn has_value(&self, id: &str) -> bool {
        self.messages.get(id).is_some_and(|m| m.value.is_some())
    }

    /// Formats a message's value. Errors (missing variables, cycles, missing references, too
    /// much expansion) return Err so the caller can fall back instead of showing partial text.
    pub(crate) fn format(&self, id: &str, args: Option<&Args<'_>>) -> Result<String, String> {
        let pattern = self
            .messages
            .get(id)
            .and_then(|m| m.value.as_ref())
            .ok_or_else(|| format!("no value for {id}"))?;
        let mut scope = Scope {
            args,
            in_term: false,
            transform: self.transform,
            placeables: 0,
            active: Vec::new(),
        };
        let mut out = String::new();
        self.enter(pattern, false, &mut scope, &mut out)?;
        Ok(out)
    }

    /// Resolves a message, term or attribute value, guarding against cycles.
    fn enter<'b>(
        &'b self,
        pattern: &'b Pattern,
        term: bool,
        scope: &mut Scope<'b, '_>,
        out: &mut String,
    ) -> Result<(), String> {
        if scope.active.iter().any(|p| ptr::eq(*p, pattern)) {
            return Err("reference cycle".into());
        }
        scope.active.push(pattern);
        let saved = (scope.args, scope.in_term);
        if term {
            scope.args = None;
            scope.in_term = true;
        }
        let result = self.pattern(pattern, scope, out);
        (scope.args, scope.in_term) = saved;
        scope.active.pop();
        result
    }

    fn pattern<'b>(
        &'b self,
        pattern: &'b Pattern,
        scope: &mut Scope<'b, '_>,
        out: &mut String,
    ) -> Result<(), String> {
        for element in &pattern.elements {
            match element {
                Element::Text(text) => match scope.transform {
                    Some(transform) => out.push_str(&transform(text)),
                    None => out.push_str(text),
                },
                Element::Placeable(expression) => self.expression(expression, scope, out)?,
            }
        }
        Ok(())
    }

    fn expression<'b>(
        &'b self,
        expression: &'b Expression,
        scope: &mut Scope<'b, '_>,
        out: &mut String,
    ) -> Result<(), String> {
        // Counted here so nested placeables ({ { "x" } }) count too.
        scope.placeables += 1;
        if scope.placeables > MAX_PLACEABLES {
            return Err(format!("more than {MAX_PLACEABLES} placeables"));
        }
        match expression {
            Expression::Inline(inline) => self.inline(inline, scope, out),
            Expression::Select { selector, variants } => {
                let chosen = self.select(selector, variants, scope)?;
                self.pattern(&chosen.value, scope, out)
            }
        }
    }

    /// Picks the first variant whose key matches, in source order as in Fluent, else the
    /// default.
    fn select<'b>(
        &'b self,
        selector: &'b Inline,
        variants: &'b [Variant],
        scope: &mut Scope<'b, '_>,
    ) -> Result<&'b Variant, String> {
        let resolved = match selector {
            Inline::Variable(name) => match scope.args.and_then(|a| a.get(name)) {
                Some(Value::Int(n)) => Some(Selector::Int(*n)),
                Some(Value::Str(s)) => Some(Selector::Str(s.to_string())),
                None if scope.in_term => None,
                None => return Err(format!("missing variable ${name}")),
            },
            Inline::String(s) => Some(Selector::Str(s.clone())),
            Inline::Number(n) => Some(Selector::Literal(n)),
            Inline::Term {
                id,
                attribute: Some(attribute),
            } => {
                let pattern = referenced(&self.terms, id, Some(attribute), "-")?;
                let mut value = String::new();
                // Selector values are compared, not shown: never pseudo-localize them.
                let transform = scope.transform.take();
                let result = self.enter(pattern, true, scope, &mut value);
                scope.transform = transform;
                result?;
                Some(Selector::Str(value))
            }
            _ => return Err("unsupported selector".into()),
        };
        let lang = self.lang;
        let int_text = match resolved {
            Some(Selector::Int(n)) => n.to_string(),
            _ => String::new(),
        };
        let matches = |key: &VariantKey| match (&resolved, key) {
            (Some(Selector::Int(_)), VariantKey::Number(k)) => decimal_eq(&int_text, k),
            (Some(Selector::Int(n)), VariantKey::Identifier(k)) => k == lang.plural_category(*n),
            (Some(Selector::Literal(n)), VariantKey::Number(k)) => decimal_eq(n, k),
            (Some(Selector::Literal(n)), VariantKey::Identifier(k)) => {
                k == lang.plural_category_literal(n)
            }
            (Some(Selector::Str(s)), VariantKey::Identifier(k)) => k == s,
            _ => false,
        };
        variants
            .iter()
            .find(|v| matches(&v.key))
            .or_else(|| variants.iter().find(|v| v.default))
            .ok_or_else(|| "select expression without a default variant".into())
    }

    fn inline<'b>(
        &'b self,
        inline: &'b Inline,
        scope: &mut Scope<'b, '_>,
        out: &mut String,
    ) -> Result<(), String> {
        match inline {
            Inline::String(s) => out.push_str(s),
            Inline::Number(n) => out.push_str(&normalize_literal(n)),
            Inline::Variable(name) => match scope.args.and_then(|a| a.get(name)) {
                Some(Value::Str(s)) => out.push_str(s),
                Some(Value::Int(n)) => out.push_str(&self.lang.format_integer(*n)),
                None => return Err(format!("missing variable ${name}")),
            },
            Inline::Message { id, attribute } => {
                let pattern = referenced(&self.messages, id, attribute.as_deref(), "")?;
                self.enter(pattern, false, scope, out)?;
            }
            Inline::Term { id, attribute } => {
                let pattern = referenced(&self.terms, id, attribute.as_deref(), "-")?;
                self.enter(pattern, true, scope, out)?;
            }
            Inline::Placeable(expression) => self.expression(expression, scope, out)?,
        }
        Ok(())
    }
}

fn referenced<'m>(
    map: &'m HashMap<String, ftl::Message>,
    id: &str,
    attribute: Option<&str>,
    sigil: &str,
) -> Result<&'m Pattern, String> {
    let message = map
        .get(id)
        .ok_or_else(|| format!("unknown reference {sigil}{id}"))?;
    match attribute {
        Some(attr) => message
            .attributes
            .iter()
            .find(|a| a.id == attr)
            .map(|a| &a.value)
            .ok_or_else(|| format!("unknown attribute {sigil}{id}.{attr}")),
        None => message
            .value
            .as_ref()
            .ok_or_else(|| format!("{sigil}{id} has no value")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args;

    fn bundle(lang: Lang, source: &str) -> Bundle {
        let (resource, errors) = ftl::parse(source);
        assert!(errors.is_empty(), "{errors:?}");
        let mut bundle = Bundle::new(lang, None);
        bundle.add(resource);
        bundle
    }

    #[test]
    fn first_matching_variant_wins() {
        let b = bundle(
            Lang::En,
            "m =\n    { $n ->\n        [0] none\n        [one] one\n       *[other] many\n    }\nr =\n    { $n ->\n        [one] category\n        [1] exact\n       *[other] many\n    }\n",
        );
        let f = |id: &str, n: i64| b.format(id, Some(&args!(n = n)));
        assert_eq!(f("m", 0).as_deref(), Ok("none"));
        assert_eq!(f("m", 1).as_deref(), Ok("one"));
        assert_eq!(f("m", -1).as_deref(), Ok("one"));
        assert_eq!(f("m", 7).as_deref(), Ok("many"));
        assert_eq!(f("r", 1).as_deref(), Ok("category"));
    }

    #[test]
    fn literal_selectors_use_cldr_operands() {
        let source = "m =\n    { 1.0 ->\n        [one] one\n       *[other] other\n    }\n";
        assert_eq!(
            bundle(Lang::En, source).format("m", None).as_deref(),
            Ok("other")
        );
        assert_eq!(
            bundle(Lang::Tr, source).format("m", None).as_deref(),
            Ok("one")
        );
    }

    #[test]
    fn string_selectors_and_defaults() {
        let b = bundle(
            Lang::En,
            "m =\n    { $kind ->\n        [voice] Voice\n       *[text] Text\n    }\n",
        );
        assert_eq!(
            b.format("m", Some(&args!(kind = "voice"))).as_deref(),
            Ok("Voice")
        );
        assert_eq!(
            b.format("m", Some(&args!(kind = "other"))).as_deref(),
            Ok("Text")
        );
    }

    #[test]
    fn references_and_terms_do_not_leak_variables() {
        let b = bundle(
            Lang::Tr,
            "-brand = Duydaş { $x }\nname = Duydaş\nhello = { name } + { $n }\nleak = { -brand }\n",
        );
        assert_eq!(
            b.format("hello", Some(&args!(n = 1234))).as_deref(),
            Ok("Duydaş + 1.234")
        );
        assert!(b.format("leak", Some(&args!(x = "y"))).is_err());
    }

    #[test]
    fn terms_take_their_default_variant_and_select_on_attributes() {
        let b = bundle(
            Lang::En,
            "-brand =\n    { $case ->\n        [genitive] Aurora's\n       *[nominative] Aurora\n    }\n    .gender = feminine\nm = { -brand }\ng =\n    { -brand.gender ->\n        [feminine] she\n       *[other] it\n    }\n",
        );
        assert_eq!(
            b.format("m", Some(&args!(case = "genitive"))).as_deref(),
            Ok("Aurora")
        );
        assert_eq!(b.format("g", None).as_deref(), Ok("she"));
    }

    #[test]
    fn pseudo_transform_skips_selector_values() {
        fn shout(text: &str) -> Cow<'_, str> {
            Cow::Owned(text.to_uppercase())
        }
        let (resource, _) = ftl::parse(
            "-t = T\n    .kind = voice\nm =\n    { -t.kind ->\n        [voice] v\n       *[other] o\n    }\n",
        );
        let mut b = Bundle::new(Lang::En, Some(shout));
        b.add(resource);
        assert_eq!(b.format("m", None).as_deref(), Ok("V"));
    }

    #[test]
    fn missing_variables_cycles_and_runaway_expansion_are_errors() {
        let mut source = String::from("a = { $x }\nb = { c }\nc = { b }\nl0 = x\n");
        for i in 1..=8 {
            source.push_str(&format!("l{i} = {{ l{p} }}{{ l{p} }}\n", p = i - 1));
        }
        source.push_str("s =\n    { $n ->\n        [0] { s }\n       *[other] fine\n    }\n");
        let b = bundle(Lang::En, &source);
        assert!(b.format("a", None).is_err());
        assert!(b.format("b", None).is_err());
        assert!(b.format("nope", None).is_err());
        // l5 resolves 62 placeables, l6 126.
        assert!(b.format("l5", None).is_ok());
        assert!(b.format("l6", None).is_err());
        assert_eq!(b.format("s", Some(&args!(n = 2))).as_deref(), Ok("fine"));
        assert!(b.format("s", Some(&args!(n = 0))).is_err());
        let nested = format!("n = {}\n", r#"{ { "x" } }"#.repeat(51));
        assert!(bundle(Lang::En, &nested).format("n", None).is_err());
        assert_eq!(
            bundle(Lang::En, "m = { 0007.50 } { -0 }\n")
                .format("m", None)
                .as_deref(),
            Ok("7.50 -0")
        );
    }
}
