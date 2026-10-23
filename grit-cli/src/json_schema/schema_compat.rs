//! Structural JSON Schema backward-compatibility checks for `--json` contracts.
//!
//! Compares a committed (older) schema document to a newly generated one and
//! reports breaking changes: removed properties, narrowed types, new required
//! fields, removed enum variants, tightened `additionalProperties`, and removed
//! nullability.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

/// One breaking change between an older and newer schema document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreakingChange {
    /// JSON pointer-style path (e.g. `/properties/status`).
    pub path: String,
    /// Short machine-readable category.
    pub kind: &'static str,
    /// Human-readable detail for test output and CI logs.
    pub detail: String,
}

/// Returns every breaking difference when `new_doc` replaces `old_doc`.
///
/// Additive changes (new optional properties, new enum variants, new definitions,
/// adding an `enum` to a former free-form string) produce an empty list.
pub fn breaking_changes(old_doc: &Value, new_doc: &Value) -> Vec<BreakingChange> {
    let old_root = SchemaNode::parse(old_doc);
    let new_root = SchemaNode::parse(new_doc);
    let mut out = Vec::new();
    compare_nodes("", &old_root, &new_root, &mut out);
    out
}

/// Formats breaking changes for a test failure or update-mode refusal.
pub fn format_breaking_changes(command: &str, version: u32, changes: &[BreakingChange]) -> String {
    let mut lines = vec![format!(
        "breaking JSON schema change for `{command}` v{version} (publish v{} and keep v{version} frozen):",
        version + 1
    )];
    for change in changes {
        lines.push(format!(
            "  {} at {}: {}",
            change.kind, change.path, change.detail
        ));
    }
    lines.join("\n")
}

#[derive(Debug, Clone)]
struct SchemaNode {
    types: TypeSet,
    properties: BTreeMap<String, SchemaNode>,
    required: BTreeSet<String>,
    additional_properties: AdditionalProperties,
    enum_values: Option<BTreeSet<String>>,
    items: Option<Box<SchemaNode>>,
    combinator: Combinator,
}

#[derive(Debug, Clone)]
enum AdditionalProperties {
    Unspecified,
    Allowed,
    Forbidden,
    Schema(Box<SchemaNode>),
}

#[derive(Debug, Clone)]
enum Combinator {
    None,
    OneOf(Vec<SchemaNode>),
    AnyOf(Vec<SchemaNode>),
    AllOf(Vec<SchemaNode>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum NumericWidth {
    #[default]
    None,
    /// `type: integer` — integral numbers only.
    IntegerOnly,
    /// `type: number` — any JSON number (including non-integer).
    AnyNumber,
}

#[derive(Debug, Clone, Default)]
struct TypeSet {
    null: bool,
    boolean: bool,
    string: bool,
    array: bool,
    object: bool,
    numeric: NumericWidth,
}

impl TypeSet {
    fn from_type_value(value: &Value) -> Self {
        let mut set = Self::default();
        match value {
            Value::String(name) => set.insert_name(name),
            Value::Array(names) => {
                for name in names {
                    if let Some(s) = name.as_str() {
                        set.insert_name(s);
                    }
                }
            }
            _ => {}
        }
        set
    }

    fn insert_name(&mut self, name: &str) {
        match name {
            "null" => self.null = true,
            "boolean" => self.boolean = true,
            "integer" => {
                self.numeric = match self.numeric {
                    NumericWidth::AnyNumber => NumericWidth::AnyNumber,
                    _ => NumericWidth::IntegerOnly,
                }
            }
            "number" => self.numeric = NumericWidth::AnyNumber,
            "string" => self.string = true,
            "array" => self.array = true,
            "object" => self.object = true,
            _ => {}
        }
    }

    fn is_empty(&self) -> bool {
        !self.null
            && !self.boolean
            && !self.string
            && !self.array
            && !self.object
            && self.numeric == NumericWidth::None
    }

    /// True when `self` accepts a strict subset of instances `old` allowed.
    fn narrows(&self, old: &Self) -> bool {
        if self.is_empty() {
            return false;
        }
        (old.null && !self.null)
            || (old.boolean && !self.boolean)
            || (old.string && !self.string)
            || (old.array && !self.array)
            || (old.object && !self.object)
            || numeric_narrows(old.numeric, self.numeric)
            || (old.numeric != NumericWidth::None && self.numeric == NumericWidth::None)
    }
}

fn numeric_narrows(old: NumericWidth, new: NumericWidth) -> bool {
    matches!(
        (old, new),
        (NumericWidth::AnyNumber, NumericWidth::IntegerOnly)
    )
}

impl SchemaNode {
    fn parse(doc: &Value) -> Self {
        let root = doc.as_object().expect("schema document must be an object");
        let definitions = root.get("definitions").cloned().unwrap_or(Value::Null);
        Self::parse_inner(doc, &definitions, 0)
    }

    fn parse_inner(value: &Value, definitions: &Value, depth: u16) -> Self {
        if depth > 32 {
            return Self::opaque();
        }
        if let Some(reference) = value.get("$ref").and_then(Value::as_str) {
            if let Some(target) = resolve_ref(reference, definitions) {
                return Self::parse_inner(&target, definitions, depth + 1);
            }
            return Self::opaque();
        }

        let mut node = Self::opaque();
        if let Some(type_value) = value.get("type") {
            node.types = TypeSet::from_type_value(type_value);
        }
        if let Some(extra) = value.get("additionalProperties") {
            node.additional_properties = match extra {
                Value::Bool(false) => AdditionalProperties::Forbidden,
                Value::Bool(true) => AdditionalProperties::Allowed,
                schema => AdditionalProperties::Schema(Box::new(Self::parse_inner(
                    schema,
                    definitions,
                    depth + 1,
                ))),
            };
        }
        if let Some(values) = value.get("enum").and_then(Value::as_array) {
            let set: BTreeSet<String> = values
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect();
            if !set.is_empty() {
                node.enum_values = Some(set);
            }
        }
        if let Some(props) = value.get("properties").and_then(Value::as_object) {
            for (name, prop) in props {
                node.properties.insert(
                    name.clone(),
                    Self::parse_inner(prop, definitions, depth + 1),
                );
            }
        }
        if let Some(required) = value.get("required").and_then(Value::as_array) {
            for name in required {
                if let Some(s) = name.as_str() {
                    node.required.insert(s.to_owned());
                }
            }
        }
        if let Some(items) = value.get("items") {
            node.items = Some(Box::new(Self::parse_inner(items, definitions, depth + 1)));
        }

        if let Some(one_of) = value.get("oneOf").and_then(Value::as_array) {
            node.combinator = Combinator::OneOf(
                one_of
                    .iter()
                    .map(|v| Self::parse_inner(v, definitions, depth + 1))
                    .collect(),
            );
        } else if let Some(any_of) = value.get("anyOf").and_then(Value::as_array) {
            node.combinator = Combinator::AnyOf(
                any_of
                    .iter()
                    .map(|v| Self::parse_inner(v, definitions, depth + 1))
                    .collect(),
            );
        } else if let Some(all_of) = value.get("allOf").and_then(Value::as_array) {
            node.combinator = Combinator::AllOf(
                all_of
                    .iter()
                    .map(|v| Self::parse_inner(v, definitions, depth + 1))
                    .collect(),
            );
        }

        node
    }

    fn opaque() -> Self {
        Self {
            types: TypeSet::default(),
            properties: BTreeMap::new(),
            required: BTreeSet::new(),
            additional_properties: AdditionalProperties::Unspecified,
            enum_values: None,
            items: None,
            combinator: Combinator::None,
        }
    }
}

fn resolve_ref(reference: &str, definitions: &Value) -> Option<Value> {
    let suffix = reference.strip_prefix("#/definitions/")?;
    definitions.get(suffix).cloned()
}

fn compare_nodes(path: &str, old: &SchemaNode, new: &SchemaNode, out: &mut Vec<BreakingChange>) {
    if compare_combinators(path, old, new, out) {
        return;
    }
    compare_node_shape(path, old, new, out);
}

/// Compare type/property shape without re-entering combinator dispatch.
fn compare_node_shape(
    path: &str,
    old: &SchemaNode,
    new: &SchemaNode,
    out: &mut Vec<BreakingChange>,
) {
    if new.types.narrows(&old.types) {
        out.push(BreakingChange {
            path: path.to_owned(),
            kind: "type_narrowed",
            detail: format!(
                "type narrowed from {} to {}",
                type_label(&old.types),
                type_label(&new.types)
            ),
        });
    }

    compare_enums(path, old, new, out);
    compare_additional_properties(path, old, new, out);
    compare_properties(path, old, new, out);
    compare_items(path, old, new, out);
}

/// Returns `true` when combinator logic fully handled this pair (no shape pass needed).
fn compare_combinators(
    path: &str,
    old: &SchemaNode,
    new: &SchemaNode,
    out: &mut Vec<BreakingChange>,
) -> bool {
    use Combinator::{AllOf, AnyOf, None as NoneCombo, OneOf};
    match (&old.combinator, &new.combinator) {
        (NoneCombo, NoneCombo) => false,
        (OneOf(old_arms), OneOf(new_arms)) | (AnyOf(old_arms), AnyOf(new_arms)) => {
            compare_combinator_arms(path, old_arms, new_arms, out);
            true
        }
        (AllOf(old_arms), AllOf(new_arms)) => {
            for (index, old_arm) in old_arms.iter().enumerate() {
                let arm_path = format!("{path}/[allOf {index}]");
                if let Some(new_arm) = new_arms.get(index) {
                    compare_nodes(&arm_path, old_arm, new_arm, out);
                }
            }
            true
        }
        (NoneCombo, OneOf(new_arms)) | (NoneCombo, AnyOf(new_arms)) => {
            for (index, new_arm) in new_arms.iter().enumerate() {
                let arm_path = format!("{path}/[arm {index}]");
                compare_node_shape(&arm_path, old, new_arm, out);
            }
            true
        }
        (OneOf(old_arms), NoneCombo) | (AnyOf(old_arms), NoneCombo) => {
            for (index, old_arm) in old_arms.iter().enumerate() {
                let arm_path = format!("{path}/[arm {index}]");
                compare_node_shape(&arm_path, old_arm, new, out);
            }
            true
        }
        _ => {
            // Mismatched combinator kinds: compare shapes once without combinator recursion.
            compare_node_shape(path, old, new, out);
            true
        }
    }
}

fn compare_combinator_arms(
    path: &str,
    old_arms: &[SchemaNode],
    new_arms: &[SchemaNode],
    out: &mut Vec<BreakingChange>,
) {
    let Some(pairs) = assign_combinator_arms(old_arms, new_arms) else {
        out.push(BreakingChange {
            path: path.to_owned(),
            kind: "combinator_narrowed",
            detail: "no complete backward-compatible assignment for combinator arms".into(),
        });
        return;
    };
    for (old_index, new_index) in pairs {
        let arm_path = format!("{path}/[arm {old_index}]");
        compare_nodes(&arm_path, &old_arms[old_index], &new_arms[new_index], out);
    }
}

/// Bipartite matching: each old arm maps to a distinct backward-compatible new arm.
fn assign_combinator_arms(
    old_arms: &[SchemaNode],
    new_arms: &[SchemaNode],
) -> Option<Vec<(usize, usize)>> {
    let mut assignment = vec![None; old_arms.len()];
    let mut used = vec![false; new_arms.len()];
    if backtrack_arm_assignment(0, old_arms, new_arms, &mut assignment, &mut used) {
        let mut pairs = Vec::with_capacity(old_arms.len());
        for (old_index, new_index) in assignment.into_iter().enumerate() {
            if let Some(new_index) = new_index {
                pairs.push((old_index, new_index));
            }
        }
        if pairs.len() == old_arms.len() {
            Some(pairs)
        } else {
            None
        }
    } else {
        None
    }
}

fn backtrack_arm_assignment(
    old_index: usize,
    old_arms: &[SchemaNode],
    new_arms: &[SchemaNode],
    assignment: &mut [Option<usize>],
    used: &mut [bool],
) -> bool {
    if old_index == old_arms.len() {
        return true;
    }
    for (new_index, new_arm) in new_arms.iter().enumerate() {
        if used[new_index] || !arm_backward_compatible(&old_arms[old_index], new_arm) {
            continue;
        }
        used[new_index] = true;
        assignment[old_index] = Some(new_index);
        if backtrack_arm_assignment(old_index + 1, old_arms, new_arms, assignment, used) {
            return true;
        }
        used[new_index] = false;
        assignment[old_index] = None;
    }
    false
}

/// Whether instances allowed by `old_arm` still validate against `new_arm`.
fn arm_backward_compatible(old_arm: &SchemaNode, new_arm: &SchemaNode) -> bool {
    breaking_changes_between_nodes(old_arm, new_arm).is_empty()
}

fn breaking_changes_between_nodes(old: &SchemaNode, new: &SchemaNode) -> Vec<BreakingChange> {
    let mut out = Vec::new();
    compare_nodes("", old, new, &mut out);
    out
}

fn compare_enums(path: &str, old: &SchemaNode, new: &SchemaNode, out: &mut Vec<BreakingChange>) {
    let (Some(old_enum), Some(new_enum)) = (&old.enum_values, &new.enum_values) else {
        if old.enum_values.is_none()
            && new.enum_values.is_some()
            && old.types.string
            && !old.types.object
        {
            return;
        }
        return;
    };
    for value in old_enum {
        if !new_enum.contains(value) {
            out.push(BreakingChange {
                path: path.to_owned(),
                kind: "enum_variant_removed",
                detail: format!("enum variant `{value}` removed"),
            });
        }
    }
}

fn compare_additional_properties(
    path: &str,
    old: &SchemaNode,
    new: &SchemaNode,
    out: &mut Vec<BreakingChange>,
) {
    let ap_path = if path.is_empty() {
        "/additionalProperties".into()
    } else {
        format!("{path}/additionalProperties")
    };

    match (&old.additional_properties, &new.additional_properties) {
        (_, AdditionalProperties::Forbidden)
            if !matches!(old.additional_properties, AdditionalProperties::Forbidden) =>
        {
            out.push(BreakingChange {
                path: path.to_owned(),
                kind: "additional_properties_tightened",
                detail: "additionalProperties became false".into(),
            });
        }
        (
            AdditionalProperties::Unspecified | AdditionalProperties::Allowed,
            AdditionalProperties::Schema(_),
        ) => {
            out.push(BreakingChange {
                path: ap_path,
                kind: "additional_properties_tightened",
                detail: "additionalProperties value schema restricted".into(),
            });
        }
        (AdditionalProperties::Schema(old_schema), AdditionalProperties::Schema(new_schema)) => {
            compare_nodes(&ap_path, old_schema, new_schema, out);
        }
        (AdditionalProperties::Schema(_), AdditionalProperties::Forbidden) => {}
        _ => {}
    }
}

fn compare_properties(
    path: &str,
    old: &SchemaNode,
    new: &SchemaNode,
    out: &mut Vec<BreakingChange>,
) {
    for (name, old_prop) in &old.properties {
        let prop_path = if path.is_empty() {
            format!("/properties/{name}")
        } else {
            format!("{path}/properties/{name}")
        };
        let Some(new_prop) = new.properties.get(name) else {
            out.push(BreakingChange {
                path: prop_path,
                kind: "property_removed",
                detail: format!("property `{name}` removed"),
            });
            continue;
        };
        compare_nodes(&prop_path, old_prop, new_prop, out);
    }

    for name in &new.required {
        if old.required.contains(name) {
            continue;
        }
        let prop_path = if path.is_empty() {
            format!("/required/{name}")
        } else {
            format!("{path}/required/{name}")
        };
        out.push(BreakingChange {
            path: prop_path,
            kind: "property_newly_required",
            detail: format!("property `{name}` is newly required"),
        });
    }
}

fn compare_items(path: &str, old: &SchemaNode, new: &SchemaNode, out: &mut Vec<BreakingChange>) {
    match (&old.items, &new.items) {
        (Some(old_items), Some(new_items)) => {
            let items_path = if path.is_empty() {
                "/items".into()
            } else {
                format!("{path}/items")
            };
            compare_nodes(&items_path, old_items, new_items, out);
        }
        (Some(_), None) => out.push(BreakingChange {
            path: if path.is_empty() {
                "/items".into()
            } else {
                format!("{path}/items")
            },
            kind: "items_removed",
            detail: "array item schema removed".into(),
        }),
        _ => {}
    }
}

fn type_label(types: &TypeSet) -> String {
    let mut names = Vec::new();
    if types.null {
        names.push("null");
    }
    if types.boolean {
        names.push("boolean");
    }
    match types.numeric {
        NumericWidth::IntegerOnly => names.push("integer"),
        NumericWidth::AnyNumber => names.push("number"),
        NumericWidth::None => {}
    }
    if types.string {
        names.push("string");
    }
    if types.array {
        names.push("array");
    }
    if types.object {
        names.push("object");
    }
    if names.is_empty() {
        "(unspecified)".into()
    } else {
        names.join("|")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn assert_breaking(old: Value, new: Value) {
        assert!(
            !breaking_changes(&old, &new).is_empty(),
            "expected breaking changes"
        );
    }

    fn assert_compatible(old: Value, new: Value) {
        assert!(
            breaking_changes(&old, &new).is_empty(),
            "unexpected breaking changes: {:?}",
            breaking_changes(&old, &new)
        );
    }

    #[test]
    fn schema_compat_removed_property() {
        let old = json!({
            "type": "object",
            "properties": {
                "status": { "type": "string" },
                "branch": { "type": "string" }
            },
            "required": ["status"]
        });
        let new = json!({
            "type": "object",
            "properties": {
                "branch": { "type": "string" }
            },
            "required": ["branch"]
        });
        let changes = breaking_changes(&old, &new);
        assert!(changes
            .iter()
            .any(|c| c.kind == "property_removed" && c.detail.contains("status")));
    }

    #[test]
    fn schema_compat_type_change() {
        let old = json!({ "type": "string" });
        let new = json!({ "type": "integer" });
        let changes = breaking_changes(&old, &new);
        assert!(changes.iter().any(|c| c.kind == "type_narrowed"));
        assert_breaking(old, new);
    }

    #[test]
    fn schema_compat_new_required() {
        let old = json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            }
        });
        let new = json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            },
            "required": ["name"]
        });
        let changes = breaking_changes(&old, &new);
        assert!(changes.iter().any(|c| c.kind == "property_newly_required"));
    }

    #[test]
    fn schema_compat_enum_removed() {
        let old = json!({ "type": "string", "enum": ["a", "b", "c"] });
        let new = json!({ "type": "string", "enum": ["a", "b"] });
        let changes = breaking_changes(&old, &new);
        assert!(changes
            .iter()
            .any(|c| c.kind == "enum_variant_removed" && c.detail.contains("c")));
    }

    #[test]
    fn schema_compat_additive_ok() {
        let old = json!({
            "type": "object",
            "properties": {
                "a": { "type": "string" }
            },
            "required": ["a"]
        });
        let new = json!({
            "type": "object",
            "properties": {
                "a": { "type": "string" },
                "b": { "type": "integer" }
            },
            "required": ["a"]
        });
        assert_compatible(old, new);
    }

    #[test]
    fn schema_compat_enum_on_string_ok() {
        let old = json!({
            "type": "object",
            "properties": {
                "kind": { "type": "string" }
            }
        });
        let new = json!({
            "type": "object",
            "properties": {
                "kind": { "type": "string", "enum": ["conflict", "plain"] }
            }
        });
        assert_compatible(old, new);
    }

    #[test]
    fn schema_compat_nullable_removed() {
        let old = json!({ "type": ["string", "null"] });
        let new = json!({ "type": "string" });
        assert_breaking(old, new);
    }

    #[test]
    fn schema_compat_resolves_ref() {
        let old = json!({
            "definitions": {
                "Item": {
                    "type": "object",
                    "properties": { "id": { "type": "string" } },
                    "required": ["id"]
                }
            },
            "type": "object",
            "properties": {
                "item": { "$ref": "#/definitions/Item" }
            }
        });
        let new = json!({
            "definitions": {
                "Item": {
                    "type": "object",
                    "properties": { "name": { "type": "string" } },
                    "required": ["name"]
                }
            },
            "type": "object",
            "properties": {
                "item": { "$ref": "#/definitions/Item" }
            }
        });
        assert_breaking(old, new);
    }

    #[test]
    fn schema_compat_additional_properties_value_type_change() {
        let old = json!({
            "type": "object",
            "additionalProperties": { "type": "string" }
        });
        let new = json!({
            "type": "object",
            "additionalProperties": { "type": "integer" }
        });
        assert_breaking(old, new);
    }

    #[test]
    fn schema_compat_anyof_arm_reorder_ok() {
        let old = json!({
            "anyOf": [
                { "type": "string" },
                { "type": "null" }
            ]
        });
        let new = json!({
            "anyOf": [
                { "type": "null" },
                { "type": "string" }
            ]
        });
        assert_compatible(old, new);
    }

    #[test]
    fn schema_compat_oneof_arm_insertion_ok() {
        let old = json!({
            "oneOf": [
                { "type": "string" },
                { "type": "null" }
            ]
        });
        let new = json!({
            "oneOf": [
                { "type": "string" },
                { "type": "null" },
                { "type": "boolean" }
            ]
        });
        assert_compatible(old, new);
    }

    #[test]
    fn schema_compat_integer_to_number_widening_ok() {
        let old = json!({ "type": "integer" });
        let new = json!({ "type": "number" });
        assert_compatible(old, new);
    }

    #[test]
    fn schema_compat_number_to_integer_narrowing() {
        let old = json!({ "type": "number" });
        let new = json!({ "type": "integer" });
        assert_breaking(old, new);
    }

    #[test]
    fn schema_compat_mismatched_combinator_does_not_stack_overflow() {
        let old = json!({ "oneOf": [{ "type": "string" }] });
        let new = json!({ "anyOf": [{ "type": "string" }] });
        let _ = breaking_changes(&old, &new);
    }

    #[test]
    fn schema_compat_anyof_overlapping_numeric_arms_reorder_ok() {
        let old = json!({
            "anyOf": [
                { "type": "integer" },
                { "type": "number" }
            ]
        });
        let new = json!({
            "anyOf": [
                { "type": "number" },
                { "type": "integer" }
            ]
        });
        assert_compatible(old, new);
    }

    #[test]
    fn schema_compat_integer_to_string_breaking() {
        let old = json!({ "type": "integer" });
        let new = json!({ "type": "string" });
        assert_breaking(old, new);
    }

    #[test]
    fn schema_compat_number_to_string_breaking() {
        let old = json!({ "type": "number" });
        let new = json!({ "type": "string" });
        assert_breaking(old, new);
    }

    #[test]
    fn schema_compat_additional_properties_unrestricted_to_schema() {
        let old = json!({
            "type": "object",
            "additionalProperties": true
        });
        let new = json!({
            "type": "object",
            "additionalProperties": { "type": "string" }
        });
        assert_breaking(old, new);
    }
}
