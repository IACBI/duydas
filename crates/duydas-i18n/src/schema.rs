//! FTL schema extraction shared by `build.rs`, the runtime crate and `xtask`.
//!
//! This file is compiled three times (via `#[path]` in `build.rs` and `xtask`), so it must only
//! depend on `std` and `fluent-syntax`.

use std::collections::BTreeSet;

use fluent_syntax::ast;

/// What a single message in an FTL file declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageInfo {
    /// Message identifier, e.g. `composer-placeholder`.
    pub id: String,
    /// Whether the message has a value (messages may consist of attributes only).
    pub has_value: bool,
    /// Variables (`$name`) referenced anywhere in the value or attributes.
    pub variables: BTreeSet<String>,
    /// Attribute names (`.tooltip`).
    pub attributes: BTreeSet<String>,
}

/// Parses FTL source and lists its messages. Terms (`-brand`) are skipped.
///
/// # Errors
/// Returns human-readable parser errors if the source is not valid FTL.
pub fn collect_messages(source: &str) -> Result<Vec<MessageInfo>, Vec<String>> {
    let resource = fluent_syntax::parser::parse(source)
        .map_err(|(_, errors)| errors.iter().map(|e| format!("{e:?}")).collect::<Vec<_>>())?;
    let mut out = Vec::new();
    for entry in &resource.body {
        if let ast::Entry::Message(msg) = entry {
            let mut variables = BTreeSet::new();
            if let Some(value) = &msg.value {
                walk_pattern(value, &mut variables);
            }
            let mut attributes = BTreeSet::new();
            for attr in &msg.attributes {
                attributes.insert(attr.id.name.to_string());
                walk_pattern(&attr.value, &mut variables);
            }
            out.push(MessageInfo {
                id: msg.id.name.to_string(),
                has_value: msg.value.is_some(),
                variables,
                attributes,
            });
        }
    }
    Ok(out)
}

/// Converts a kebab-case message id into the generated `Key` variant name
/// (`composer-placeholder` → `ComposerPlaceholder`).
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

fn walk_pattern(pattern: &ast::Pattern<&str>, vars: &mut BTreeSet<String>) {
    for element in &pattern.elements {
        if let ast::PatternElement::Placeable { expression } = element {
            walk_expression(expression, vars);
        }
    }
}

fn walk_expression(expression: &ast::Expression<&str>, vars: &mut BTreeSet<String>) {
    match expression {
        ast::Expression::Select { selector, variants } => {
            walk_inline(selector, vars);
            for variant in variants {
                walk_pattern(&variant.value, vars);
            }
        }
        ast::Expression::Inline(inline) => walk_inline(inline, vars),
    }
}

fn walk_inline(inline: &ast::InlineExpression<&str>, vars: &mut BTreeSet<String>) {
    match inline {
        ast::InlineExpression::VariableReference { id } => {
            vars.insert(id.name.to_string());
        }
        ast::InlineExpression::FunctionReference { arguments, .. } => walk_args(arguments, vars),
        ast::InlineExpression::TermReference {
            arguments: Some(arguments),
            ..
        } => walk_args(arguments, vars),
        ast::InlineExpression::Placeable { expression } => walk_expression(expression, vars),
        _ => {}
    }
}

fn walk_args(arguments: &ast::CallArguments<&str>, vars: &mut BTreeSet<String>) {
    for positional in &arguments.positional {
        walk_inline(positional, vars);
    }
    for named in &arguments.named {
        walk_inline(&named.value, vars);
    }
}
