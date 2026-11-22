//! FTL schema extraction shared by `build.rs`, the runtime crate and `xtask`.
//!
//! `build.rs` compiles this file together with `ftl.rs` via `#[path]`, so it may only depend
//! on std and `crate::ftl`. The build validates every locale with [`validate`], so a release binary
//! cannot embed FTL that fails it.

use std::collections::{BTreeMap, BTreeSet};

use crate::ftl::{self, Element, Entry, Expression, Inline, Message, Pattern, Resource};

/// What a single message in an FTL file declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageInfo {
    /// Message identifier, e.g. `composer-placeholder`.
    pub id: String,
    /// Whether the message has a value (messages may consist of attributes only).
    pub has_value: bool,
    /// Variables referenced anywhere in the value or attributes.
    pub variables: BTreeSet<String>,
    /// Attribute names.
    pub attributes: BTreeSet<String>,
}

/// Parses FTL source and lists its messages. Terms are skipped.
///
/// # Errors
/// Returns one line per syntax error (`line:column: message`).
pub fn collect_messages(source: &str) -> Result<Vec<MessageInfo>, Vec<String>> {
    let (resource, errors) = ftl::parse(source);
    if !errors.is_empty() {
        return Err(errors.iter().map(ToString::to_string).collect());
    }
    let mut out = Vec::new();
    for entry in &resource.entries {
        if let Entry::Message(msg) = entry {
            let mut variables = BTreeSet::new();
            if let Some(value) = &msg.value {
                walk_pattern(value, &mut variables);
            }
            let mut attributes = BTreeSet::new();
            for attr in &msg.attributes {
                attributes.insert(attr.id.clone());
                walk_pattern(&attr.value, &mut variables);
            }
            out.push(MessageInfo {
                id: msg.id.clone(),
                has_value: msg.value.is_some(),
                variables,
                attributes,
            });
        }
    }
    Ok(out)
}

/// Converts a kebab-case message id into the generated `Key` variant name
/// (`composer-placeholder` becomes `ComposerPlaceholder`).
pub fn variant_name(id: &str) -> String {
    id.split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_ascii_uppercase().to_string() + chars.as_str()
            })
        })
        .collect()
}

/// A reference target: (is a term, id, attribute).
type Node<'r> = (bool, &'r str, Option<&'r str>);

/// Checks one locale's parsed files for duplicate ids, references to missing messages, terms or
/// attributes, and reference cycles in any variant (not only the one a test happens to render).
///
/// Returns one line per problem (`file:line: problem`), sorted; empty means valid.
pub fn validate(files: &[(&str, &Resource)]) -> Vec<String> {
    let mut errors = Vec::new();
    let mut defined: BTreeMap<(bool, &str), (&str, &Message)> = BTreeMap::new();
    for (file, resource) in files {
        for entry in &resource.entries {
            let (is_term, message) = match entry {
                Entry::Message(m) => (false, m),
                Entry::Term(t) => (true, t),
            };
            let mut seen = BTreeSet::new();
            for attribute in &message.attributes {
                if !seen.insert(attribute.id.as_str()) {
                    errors.push(format!(
                        "{file}:{}: duplicate attribute {}",
                        message.line,
                        display(&(is_term, &message.id, Some(&attribute.id)))
                    ));
                }
            }
            let key = (is_term, message.id.as_str());
            if let Some((first_file, first)) = defined.get(&key) {
                errors.push(format!(
                    "{file}:{}: duplicate {} (first defined at {first_file}:{})",
                    message.line,
                    display(&(is_term, &message.id, None)),
                    first.line
                ));
            } else {
                defined.insert(key, (file, message));
            }
        }
    }

    let mut graph: BTreeMap<Node<'_>, Vec<Node<'_>>> = BTreeMap::new();
    for (&(is_term, id), &(file, message)) in &defined {
        let patterns = message.value.iter().map(|p| (None, p)).chain(
            message
                .attributes
                .iter()
                .map(|a| (Some(a.id.as_str()), &a.value)),
        );
        for (attribute, pattern) in patterns {
            let source = (is_term, id, attribute);
            let mut targets = Vec::new();
            refs_in_pattern(pattern, &mut targets);
            for target in &targets {
                let (target_is_term, target_id, target_attribute) = *target;
                let problem = match (defined.get(&(target_is_term, target_id)), target_attribute) {
                    (None, _) => Some("unknown reference"),
                    (Some((_, m)), Some(a)) if !m.attributes.iter().any(|x| x.id == a) => {
                        Some("unknown attribute")
                    }
                    (Some((_, m)), None) if m.value.is_none() => {
                        Some("reference to a message without a value:")
                    }
                    _ => None,
                };
                if let Some(problem) = problem {
                    errors.push(format!(
                        "{file}:{}: {}: {problem} {}",
                        message.line,
                        display(&source),
                        display(target)
                    ));
                }
            }
            graph.insert(source, targets);
        }
    }

    find_cycles(&graph, &mut errors);
    errors.sort();
    errors.dedup();
    errors
}

/// Iterative depth-first search (a long reference chain must not overflow the stack). Reports
/// every node at which a back edge closes a cycle.
fn find_cycles(graph: &BTreeMap<Node<'_>, Vec<Node<'_>>>, errors: &mut Vec<String>) {
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        OnStack,
        Done,
    }
    let mut state = BTreeMap::new();
    for root in graph.keys() {
        if state.contains_key(root) {
            continue;
        }
        state.insert(*root, State::OnStack);
        // (node, index of the next edge to follow)
        let mut stack = vec![(*root, 0_usize)];
        while let Some((node, next)) = stack.last_mut() {
            let edges = graph.get(node).map_or(&[][..], Vec::as_slice);
            let Some(target) = edges.get(*next).copied() else {
                state.insert(*node, State::Done);
                stack.pop();
                continue;
            };
            *next += 1;
            match state.get(&target) {
                Some(State::Done) => {}
                Some(State::OnStack) => {
                    errors.push(format!("reference cycle through {}", display(&target)));
                }
                None => {
                    state.insert(target, State::OnStack);
                    stack.push((target, 0));
                }
            }
        }
    }
}

fn display(node: &Node<'_>) -> String {
    let (is_term, id, attribute) = node;
    let sigil = if *is_term { "-" } else { "" };
    match attribute {
        Some(attribute) => format!("{sigil}{id}.{attribute}"),
        None => format!("{sigil}{id}"),
    }
}

fn refs_in_pattern<'r>(pattern: &'r Pattern, out: &mut Vec<Node<'r>>) {
    for element in &pattern.elements {
        if let Element::Placeable(expression) = element {
            refs_in_expression(expression, out);
        }
    }
}

fn refs_in_expression<'r>(expression: &'r Expression, out: &mut Vec<Node<'r>>) {
    match expression {
        Expression::Select { selector, variants } => {
            refs_in_inline(selector, out);
            for variant in variants {
                refs_in_pattern(&variant.value, out);
            }
        }
        Expression::Inline(inline) => refs_in_inline(inline, out),
    }
}

fn refs_in_inline<'r>(inline: &'r Inline, out: &mut Vec<Node<'r>>) {
    match inline {
        Inline::Message { id, attribute } => out.push((false, id, attribute.as_deref())),
        Inline::Term { id, attribute } => out.push((true, id, attribute.as_deref())),
        Inline::Placeable(expression) => refs_in_expression(expression, out),
        _ => {}
    }
}

fn walk_pattern(pattern: &Pattern, vars: &mut BTreeSet<String>) {
    for element in &pattern.elements {
        if let Element::Placeable(expression) = element {
            walk_expression(expression, vars);
        }
    }
}

fn walk_expression(expression: &Expression, vars: &mut BTreeSet<String>) {
    match expression {
        Expression::Select { selector, variants } => {
            walk_inline(selector, vars);
            for variant in variants {
                walk_pattern(&variant.value, vars);
            }
        }
        Expression::Inline(inline) => walk_inline(inline, vars),
    }
}

fn walk_inline(inline: &Inline, vars: &mut BTreeSet<String>) {
    match inline {
        Inline::Variable(id) => {
            vars.insert(id.clone());
        }
        Inline::Placeable(expression) => walk_expression(expression, vars),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(files: &[(&str, &str)]) -> Vec<String> {
        let parsed: Vec<_> = files
            .iter()
            .map(|(name, source)| {
                let (resource, errors) = ftl::parse(source);
                assert!(errors.is_empty(), "{errors:?}");
                (*name, resource)
            })
            .collect();
        let views: Vec<_> = parsed.iter().map(|(n, r)| (*n, r)).collect();
        validate(&views)
    }

    #[test]
    fn valid_files_pass() {
        let errors = check(&[
            (
                "a.ftl",
                "-brand = D\n    .kind = app\nm = { -brand } { n.title }\n",
            ),
            (
                "b.ftl",
                "n = N\n    .title = T\nk =\n    { -brand.kind ->\n        [app] a\n       *[other] o\n    }\n",
            ),
        ]);
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn duplicates_across_files() {
        let errors = check(&[("a.ftl", "m = 1\n"), ("b.ftl", "\nm = 2\n")]);
        assert_eq!(errors, ["b.ftl:2: duplicate m (first defined at a.ftl:1)"]);
    }

    #[test]
    fn dangling_references() {
        let errors = check(&[(
            "a.ftl",
            "m = { missing } { n.x } { attrs }\nn = N\nattrs =\n    .a = A\n",
        )]);
        assert_eq!(errors.len(), 3, "{errors:?}");
    }

    #[test]
    fn duplicate_attributes() {
        let errors = check(&[(
            "a.ftl",
            "a = A\n    .label = { a.label }\n    .label = OK\n",
        )]);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("duplicate attribute a.label")),
            "{errors:?}"
        );
    }

    #[test]
    fn long_chains_do_not_overflow() {
        let mut source = String::new();
        for i in 0..100_000 {
            source.push_str(&format!("m{i} = {{ m{} }}\n", i + 1));
        }
        source.push_str("m100000 = OK\n");
        assert!(check(&[("a.ftl", &source)]).is_empty());
    }

    #[test]
    fn cycles_in_any_variant() {
        let errors = check(&[(
            "a.ftl",
            "s =\n    { $n ->\n        [0] { s }\n       *[other] fine\n    }\na = { b }\n    .x = { b }\nb = { a.x }\n",
        )]);
        assert!(
            errors.iter().any(|e| e.contains("cycle through s")),
            "{errors:?}"
        );
        assert!(
            errors
                .iter()
                .any(|e| e.contains("cycle through a.x") || e.contains("cycle through b")),
            "{errors:?}"
        );
        assert!(errors.iter().all(|e| e.contains("cycle")), "{errors:?}");
    }
}
