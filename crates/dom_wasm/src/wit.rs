//! `machopo:dom` as WIT, generated from `dom_api`'s declaration
//! table: what a guest imports to reach the DOM. wasmtime does the
//! canonical ABI; this only says what the interface is.
//!
//! - **Nodes**: one type, `node`, the store's own number for a node
//!   (`handles`); each interface is a name for it (`type element = node`),
//!   for the bindings' types only: a node of another interface where one is
//!   asked for is the call's `TypeError`, as JS's receiver check.
//! - **Members**: each once, on the interface that declares it, named
//!   `<interface>-<member>` (`node-first-child`, `element-get-attribute`),
//!   an attribute's setter `<interface>-set-<attribute>`, `this` first.
//!   Each returns `result<T, dom-exception>`. Constants aren't there (WIT
//!   has none): a guest compares the numbers.
//! - **Strings**: `string`, a `DOMString?` an `option<string>` (as a
//!   `[LegacyNullToEmptyString]` argument, whose none is the empty string).
//! - `create-document` makes an HTML document; `release` lets a node go.
//! - **Plain members only** (`is_plain`): a node interface's members that
//!   take and give nodes, strings, numbers and booleans. The others (a
//!   constructor, an interface whose objects aren't nodes, a member with a
//!   dictionary, a union, a callback or a sequence) aren't there: a guest
//!   can't hold what they take or give yet.
//! - **Frozen** (`FROZEN`): the plain members as they were on 2026-10-30;
//!   the table's later ones aren't added until wasm is looked at again.

use std::fmt::Write;

use dom_api::{INTERFACES, InterfaceDesc, Member, MemberKind, TypeDesc};

/// The WIT package.
pub const PACKAGE: &str = "machopo:dom@0.1.0";

/// The interface a guest imports, by its full name.
pub const INTERFACE: &str = "machopo:dom/dom@0.1.0";

/// WIT's keywords: a name that is one is written `%name`.
const KEYWORDS: &[&str] = &[
    "as",
    "async",
    "bool",
    "borrow",
    "char",
    "constructor",
    "enum",
    "error-context",
    "export",
    "f32",
    "f64",
    "flags",
    "from",
    "func",
    "future",
    "import",
    "include",
    "interface",
    "list",
    "option",
    "own",
    "package",
    "record",
    "resource",
    "result",
    "s16",
    "s32",
    "s64",
    "s8",
    "static",
    "stream",
    "string",
    "tuple",
    "type",
    "u16",
    "u32",
    "u64",
    "u8",
    "use",
    "variant",
    "with",
    "world",
];

/// A kebab-case `name` as a WIT identifier: escaped if it is a keyword
/// (`insertAdjacentHTML`'s `string` is `%string`).
fn identifier(name: &str) -> String {
    if KEYWORDS.contains(&name) {
        format!("%{name}")
    } else {
        name.to_owned()
    }
}

/// `name` in kebab-case: `appendChild` is `append-child`,
/// `namespaceURI` `namespace-uri`, `CharacterData` `character-data`.
pub fn kebab_case(name: &str) -> String {
    // HTML's `IFrame` is one word (`iframe`), not an acronym's `I`.
    let name = name.replace("IFrame", "Iframe");
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::with_capacity(name.len().saturating_add(4));
    for (i, &c) in chars.iter().enumerate() {
        if c.is_ascii_uppercase() {
            let boundary = match i.checked_sub(1).map(|j| chars[j]) {
                Some(prev) if prev.is_ascii_lowercase() || prev.is_ascii_digit() => true,
                // The last letter of an acronym starts the next word.
                Some(prev) if prev.is_ascii_uppercase() => {
                    chars[i..].get(1).is_some_and(char::is_ascii_lowercase)
                }
                _ => false,
            };
            if boundary {
                out.push('-');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// The WIT function of `interface`'s member `member` of `kind`.
pub fn function_name(interface: &str, member: &str, kind: MemberKind) -> String {
    let set = if kind == MemberKind::Setter {
        "set-"
    } else {
        ""
    };
    format!("{}-{set}{}", kebab_case(interface), kebab_case(member))
}

/// The members a guest has, by their functions' names: the plain ones as
/// they were on 2026-10-30, frozen (the DOM's later members are JS's
/// alone until wasm is looked at again). Each is plain still (a test).
pub const FROZEN: &[&str] = &[
    "node-node-type",
    "node-node-name",
    "node-is-connected",
    "node-owner-document",
    "node-parent-node",
    "node-parent-element",
    "node-has-child-nodes",
    "node-first-child",
    "node-last-child",
    "node-previous-sibling",
    "node-next-sibling",
    "node-text-content",
    "node-set-text-content",
    "node-contains",
    "node-insert-before",
    "node-append-child",
    "node-remove-child",
    "document-document-element",
    "document-ready-state",
    "document-create-element",
    "document-create-element-ns",
    "document-create-text-node",
    "document-get-element-by-id",
    "document-first-element-child",
    "document-last-element-child",
    "document-child-element-count",
    "document-query-selector",
    "document-type-name",
    "document-type-public-id",
    "document-type-system-id",
    "document-fragment-get-element-by-id",
    "document-fragment-first-element-child",
    "document-fragment-last-element-child",
    "document-fragment-child-element-count",
    "document-fragment-query-selector",
    "element-namespace-uri",
    "element-prefix",
    "element-local-name",
    "element-tag-name",
    "element-id",
    "element-set-id",
    "element-class-name",
    "element-set-class-name",
    "element-has-attributes",
    "element-get-attribute",
    "element-get-attribute-ns",
    "element-set-attribute",
    "element-set-attribute-ns",
    "element-remove-attribute",
    "element-remove-attribute-ns",
    "element-has-attribute",
    "element-has-attribute-ns",
    "element-inner-html",
    "element-set-inner-html",
    "element-outer-html",
    "element-set-outer-html",
    "element-insert-adjacent-html",
    "element-closest",
    "element-matches",
    "element-webkit-matches-selector",
    "element-first-element-child",
    "element-last-element-child",
    "element-child-element-count",
    "element-query-selector",
    "character-data-data",
    "character-data-set-data",
    "character-data-length",
    "processing-instruction-target",
];

/// Whether `member` of `interface` is in the WIT: plain (`is_plain`), and
/// one of the frozen ones.
pub fn in_wit(interface: &InterfaceDesc, member: &Member) -> bool {
    is_plain(interface, member)
        && is_frozen(&function_name(interface.name, member.name, member.kind))
}

/// Whether the function `name` is one of the frozen members'.
pub fn is_frozen(name: &str) -> bool {
    FROZEN.contains(&name)
}

/// Whether `member` of `interface` is plain: `for_each_member!`'s `plain`
/// class, of types a guest can hold.
pub fn is_plain(interface: &InterfaceDesc, member: &Member) -> bool {
    let plain_type = |ty: TypeDesc| match ty {
        TypeDesc::Node(name) | TypeDesc::NullableNode(name) => {
            name != "ShadowRoot"
                && INTERFACES
                    .iter()
                    .any(|interface| interface.name == name && interface.node)
        }
        TypeDesc::Dictionary(_)
        | TypeDesc::DictOrBool(_)
        | TypeDesc::Callback(_)
        | TypeDesc::Function(_)
        | TypeDesc::Sequence(_)
        | TypeDesc::FrozenArray(_)
        | TypeDesc::Doubles
        | TypeDesc::Numbers
        | TypeDesc::FrozenStrings
        | TypeDesc::ObservableArray(_)
        | TypeDesc::Promise(_)
        | TypeDesc::NullableUnion(_)
        | TypeDesc::Union(_)
        | TypeDesc::Any
        | TypeDesc::Rest
        | TypeDesc::Strings
        | TypeDesc::Slottables
        | TypeDesc::NodesOrStrings
        | TypeDesc::StringPairs
        | TypeDesc::Uint8ClampedArray
        | TypeDesc::StringOr(_) => false,
        _ => true,
    };
    // An element interface's: a guest's `this` is checked by its kind
    // alone.
    // An attribute's getter and setter alike (a `[PutForwards]` setter
    // takes a string, its getter gives an object): a guest has an
    // attribute whole or not at all.
    let attribute = matches!(member.kind, MemberKind::Getter | MemberKind::Setter);
    let types = |member: &Member| {
        member
            .params
            .iter()
            .map(|param| param.ty)
            .chain([member.returns])
            .collect::<Vec<_>>()
    };
    interface.node
        && interface.element.is_none()
        // A shadow root's members: its kind, a fragment's, doesn't tell it.
        && interface.name != "ShadowRoot"
        && !matches!(
            member.kind,
            MemberKind::Constructor | MemberKind::IndexedSetter | MemberKind::LegacyFactory
        )
        // A static operation has no `this` to be a guest's handle.
        && !member.is_static
        && interface
            .members
            .iter()
            .filter(|other| {
                std::ptr::eq(*other, member)
                    || (attribute
                        && other.name == member.name
                        && matches!(other.kind, MemberKind::Getter | MemberKind::Setter))
            })
            .flat_map(types)
            .all(plain_type)
}

fn wit_type(ty: TypeDesc) -> String {
    match ty {
        TypeDesc::Undefined => "_".to_owned(),
        TypeDesc::Bool => "bool".to_owned(),
        TypeDesc::OptionalBool => "option<bool>".to_owned(),
        TypeDesc::U32 => "u32".to_owned(),
        TypeDesc::NullableU32 => "option<u32>".to_owned(),
        TypeDesc::U16 => "u16".to_owned(),
        TypeDesc::I16 => "s16".to_owned(),
        TypeDesc::I32 => "s32".to_owned(),
        TypeDesc::Double => "f64".to_owned(),
        TypeDesc::Dictionary(name)
        | TypeDesc::DictOrBool(name)
        | TypeDesc::Callback(name)
        | TypeDesc::Function(name)
        | TypeDesc::Sequence(name)
        | TypeDesc::FrozenArray(name)
        | TypeDesc::ObservableArray(name)
        | TypeDesc::Promise(name) => unreachable!("no plain member takes or gives {name}"),
        TypeDesc::NullableUnion(_) | TypeDesc::Union(_) | TypeDesc::StringOr(_) => {
            unreachable!("no plain member takes or gives a union")
        }
        TypeDesc::Uint8ClampedArray => unreachable!("no plain member takes or gives a typed array"),
        TypeDesc::Any
        | TypeDesc::Rest
        | TypeDesc::Strings
        | TypeDesc::Numbers
        | TypeDesc::FrozenStrings
        | TypeDesc::Doubles
        | TypeDesc::Slottables
        | TypeDesc::NodesOrStrings
        | TypeDesc::StringPairs => unreachable!("no plain member takes or gives any value"),
        TypeDesc::String => "string".to_owned(),
        TypeDesc::NullableString => "option<string>".to_owned(),
        TypeDesc::Node(interface) => kebab_case(interface),
        TypeDesc::NullableNode(interface) => format!("option<{}>", kebab_case(interface)),
    }
}

/// The package's WIT.
pub fn describe() -> String {
    let mut wit = String::new();
    let mut line = |text: &str| {
        wit.push_str(text);
        wit.push('\n');
    };
    line("// machopo:dom: the DOM for wasm guests, generated from dom_api's");
    line("// declaration table by dom_wasm (`wit::describe`;");
    line("// `MACHOPO_UPDATE_GOLDEN=1 cargo test -p dom_wasm --test it wit` writes it).");
    line(&format!("package {PACKAGE};"));
    line("");
    line("interface dom {");
    line("  /// A node: the store's own number for it, the same each time the store");
    line("  /// is given it, until `release` lets it go. A number that isn't one of the");
    line("  /// store's traps the call.");
    line("  type node = u32;");
    for interface in INTERFACES
        .iter()
        .filter(|interface| interface.node && interface.name != "Node")
    {
        line(&format!("  type {} = node;", kebab_case(interface.name)));
    }
    line("");
    line("  /// What a call failed with: `code` 1 is a TypeError (`this` of another");
    line("  /// interface), the others DOMException names: 2 HierarchyRequestError,");
    line("  /// 3 NotFoundError, 4 InvalidCharacterError, 9 NamespaceError.");
    line("  record dom-exception {");
    line("    code: u32,");
    line("    message: string,");
    line("  }");
    line("");
    line("  /// A new HTML document, in no window.");
    line("  create-document: func() -> document;");
    line("  /// Lets the store's node `handle` go: the number may be another node's");
    line("  /// after, and the node is collected once nothing else holds it.");
    line("  release: func(handle: node);");
    for interface in INTERFACES {
        for member in interface.members {
            if matches!(member.kind, MemberKind::Constant(_)) || !in_wit(interface, member) {
                continue;
            }
            let mut params = format!("this: {}", kebab_case(interface.name));
            for param in member.params {
                let _ = write!(
                    params,
                    ", {}: {}",
                    identifier(&kebab_case(param.name)),
                    wit_type(param.ty)
                );
            }
            let what = match member.kind {
                MemberKind::Setter => " (setter)",
                _ => "",
            };
            line("");
            line(&format!("  /// `{}.{}`{what}", interface.name, member.name));
            line(&format!(
                "  {}: func({params}) -> result<{}, dom-exception>;",
                function_name(interface.name, member.name, member.kind),
                wit_type(member.returns)
            ));
        }
    }
    line("}");
    wit
}

#[cfg(test)]
mod tests {
    use super::{identifier, kebab_case};

    #[test]
    fn keywords_are_escaped() {
        assert_eq!(identifier("string"), "%string");
        assert_eq!(identifier("data"), "data");
    }

    #[test]
    fn names_are_kebab_case() {
        for (name, kebab) in [
            ("appendChild", "append-child"),
            ("namespaceURI", "namespace-uri"),
            ("createElementNS", "create-element-ns"),
            ("CharacterData", "character-data"),
            ("nodeType", "node-type"),
            ("id", "id"),
            ("Node", "node"),
        ] {
            assert_eq!(kebab_case(name), kebab, "{name}");
        }
    }
}
