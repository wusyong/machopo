//! The declaration table (`declare!`, a proc macro of
//! `dom_api_macros`): each interface's members as WebIDL names them,
//! the function of `dom_api` each calls, and a typed path for each
//! (`crate::typed`), which a front end binds. One table for every engine
//! (V8's templates, a wasm guest's interface), as Blink's bindings
//! generator reads its IDL. An engine whose calls need each member's own
//! types (wasmtime's typed host functions) expands them from
//! `for_each_member!`, which the table generates too.
//!
//! ```text
//! interface Name : Parent [NodeKind, ...] {
//!     const NAME: u32 = value;
//!     constructor(arg: Type, optional?: Type) = module::function;
//!     attribute webIdlName: Type, get = module::getter;
//!     attribute webIdlName: Type, get = module::getter, set = module::setter;
//!     method webIdlName(arg: Type, ...) -> Type = module::function;
//!     method webIdlName(arg: Type, ...) = module::function;
//! }
//! interface Name : Parent [type = path::Type] { ... }
//!
//! dictionary Name {
//!     member: Type = default,
//! }
//! ```
//!
//! A node interface's brackets list the node kinds whose most derived
//! interface it is; it accepts its own and its descendants'
//! (`Interface::accepts`). Another interface's name the type its objects
//! are (`typed::ObjectType`: its `this`). A name the table gives that is
//! Rust's keyword is written raw (`r#type`). An optional argument (`?`) is
//! one whose type converts undefined to its default (a dictionary's).
//! A constructor's function takes the operation and its arguments, and
//! returns the new object (into a `dom::ObjectRef`), which the front
//! end wraps with the call's own new wrapper. A dictionary is a struct of
//! its members, read in their names' order (WebIDL's) before the
//! operation.
//! A function takes the operation (`ApiCx`), `this` and its arguments as
//! their types give them (`ArgType::Param`), and returns
//! `Result<_, ApiError>` of what becomes its result's (`RetType::Ret`, by
//! `Into`: a string result is a `DomString`, or a `StrRef` to give the DOM's
//! own with no clone). A getter runs in
//! an operation with no safepoint (`Dom::read`); a setter or a method in
//! one with (`Dom::mutate`). A result `NewObject<Interface>` is WebIDL's
//! `[NewObject]`: a node the member always makes.

use dom::NodeKind;

/// An interface of the table.
pub trait Interface {
    const DESC: &'static InterfaceDesc;
    /// Its name, `DESC`'s (which a type of one of its own members names
    /// it by: `DESC` is made of them).
    const NAME: &'static str;
    /// Its index in `INTERFACES`.
    const INDEX: usize;
    /// What its objects are.
    type Object: crate::typed::ObjectType;

    /// Whether a node of `kind` is one of its objects.
    fn accepts(kind: NodeKind) -> bool {
        crate::typed::accepts(Self::INDEX, kind)
    }
}

/// What the table says of an interface.
#[derive(Debug)]
pub struct InterfaceDesc {
    pub name: &'static str,
    pub parent: Option<&'static str>,
    /// Whether its objects are nodes (`[NodeKind, ...]` in the table);
    /// else they are of the GC type the table names (`[type = Type]`).
    pub node: bool,
    /// The node kinds whose most derived interface it is.
    pub kinds: &'static [NodeKind],
    /// The elements whose most derived interface it is, by name (`[element
    /// html]`, `[element html meta]`): an HTML element's interface by its
    /// local name, Blink's element factory.
    pub element: Option<ElementDesc>,
    /// WebIDL's value iterator (`#[iterable]`): `entries`, `forEach`,
    /// `keys` and `values` are `Array.prototype`'s.
    pub iterable: bool,
    /// WebIDL's `[LegacyOverrideBuiltIns]` (`#[override_builtins]`): its
    /// named properties shadow the object's own and its prototypes'.
    pub override_builtins: bool,
    /// WebIDL's namespace (`#[namespace]`): a plain object of the global
    /// (no interface object, no prototype), its members static operations.
    pub namespace: bool,
    /// WebIDL's `[Global]` (`#[global]`, `Window`): its members are the
    /// global object's own properties, not its prototype's.
    pub global: bool,
    /// HTML's [[IsHTMLDDA]] (`#[is_html_dda]`, `HTMLAllCollection`): its
    /// objects are undetectable in JS (`typeof` `"undefined"`, `== null`,
    /// falsy; V8's undetectable objects, as Blink makes them), and calling
    /// one calls its `item` (its `legacycaller`).
    pub is_html_dda: bool,
    pub members: &'static [Member],
}

/// A callback interface with constants (`NodeFilter`): WebIDL's legacy
/// callback interface object is a function of the global, without a
/// `prototype`, that throws when called, with them.
#[derive(Debug)]
pub struct CallbackInterfaceDesc {
    pub name: &'static str,
    pub constants: &'static [(&'static str, u32)],
}

/// An element interface's elements: HTML or SVG (`svg`) elements of its
/// local names (`h1`...`h6` one interface's), or, with none, those of no
/// other's (`HTMLElement`, `SVGElement`).
#[derive(Debug, Clone, Copy)]
pub struct ElementDesc {
    pub svg: bool,
    pub locals: &'static [&'static str],
}

/// A member: a constant, an attribute's getter or setter, or a method.
#[derive(Debug)]
pub struct Member {
    pub name: &'static str,
    pub kind: MemberKind,
    pub params: &'static [Param],
    pub returns: TypeDesc,
    /// WebIDL's `[LegacyUnforgeable]` (`#[unforgeable]` in the table): on
    /// each object, not its interface's prototype, and not configurable.
    pub unforgeable: bool,
    /// WebIDL's `[Replaceable]` (`#[replaceable]`, a read-only attribute's
    /// getter): assigning it defines a data property of the object, which
    /// shadows the attribute.
    pub replaceable: bool,
    /// The interface's indexed getter (`#[indexed]`: a method taking an
    /// index, giving null past the last): `object[i]` calls it. An
    /// anonymous one (`#[indexed(getter)]`, WebIDL's `getter T (unsigned
    /// long index)`) has an empty name: not on the prototype.
    pub indexed: bool,
    /// One of the interface's named properties' special operations
    /// (`#[named(..)]`), anonymous: not on the prototype.
    pub named: Option<Named>,
    /// One of an observable array attribute's backing list's operations
    /// (`#[observable(..)]`), anonymous: what the attribute's array calls.
    pub observable: Option<Observable>,
    /// WebIDL's `static` operation (`#[static_method]`): on the interface
    /// object, with no `this`.
    pub is_static: bool,
    /// The interface's pair iterator's pair at an index (`#[pairs]`,
    /// WebIDL's `iterable<K, V>`): `[key, value]`, none past the last;
    /// anonymous, what its `entries`, `keys`, `values`, `forEach` and
    /// `@@iterator` call each step.
    pub pairs: bool,
}

/// An observable array attribute's backing list's operation (WebIDL's
/// "set the indexed value" and "delete the indexed value"), with the
/// attribute's name; the attribute's getter gives the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observable {
    /// Taking an index (no greater than the length) and a value: the one
    /// there replaced (its delete algorithm run, then the value's set
    /// algorithm), or one more at the length.
    Set(&'static str),
    /// The last value taken off (its delete algorithm run), of a list that
    /// isn't empty.
    DeleteLast(&'static str),
}

/// A named property's special operation (WebIDL's named getter, setter and
/// deleter, and its supported property names).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Named {
    /// `object[name]`: taking the name, giving none if there is no such
    /// property.
    Getter,
    /// `object[name] = value`: taking the name and the value; it takes
    /// every name.
    Setter,
    /// `delete object[name]`, of a property the getter gives.
    Deleter,
    /// The names, in order: the object's own keys (`Names`).
    Names,
}

impl Member {
    /// Its required parameters (a JS function's `length`): those before
    /// the first optional one.
    pub fn required(&self) -> usize {
        self.params
            .iter()
            .position(|param| param.optional)
            .unwrap_or(self.params.len())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberKind {
    Constant(u32),
    Getter,
    Setter,
    Method,
    /// WebIDL's constructor: the interface object called with `new`.
    Constructor,
    /// WebIDL's indexed setter (`object[i] = value`): taking the index and
    /// the value; anonymous.
    IndexedSetter,
    /// WebIDL's `[LegacyFactoryFunction]`: a function of the global, named
    /// as the member, called with `new` (`new Option(...)`), giving the
    /// interface's object it makes.
    LegacyFactory,
}

#[derive(Debug)]
pub struct Param {
    pub name: &'static str,
    pub ty: TypeDesc,
    /// WebIDL's `optional` (`name?: Type`): a missing argument is
    /// undefined, which the type converts to its default.
    pub optional: bool,
}

/// A type of the table, as an engine sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeDesc {
    Undefined,
    Bool,
    /// An optional `boolean` with no default: none if it isn't given.
    OptionalBool,
    U32,
    /// WebIDL's `unsigned long?`.
    NullableU32,
    U16,
    /// WebIDL's `short`.
    I16,
    /// WebIDL's `long`.
    I32,
    Double,
    String,
    NullableString,
    /// A dictionary, by its name.
    Dictionary(&'static str),
    /// A union of a dictionary (by its name) and a `boolean`.
    DictOrBool(&'static str),
    /// A callback interface, by its name.
    Callback(&'static str),
    /// A callback function, by its name (`TimerHandler`'s function).
    Function(&'static str),
    /// Any JS value (a result): WebIDL's `any`.
    Any,
    /// The arguments from one on, any values: WebIDL's variadic `any...`.
    Rest,
    /// The arguments from one on, strings: WebIDL's variadic `DOMString...`.
    Strings,
    /// The arguments from one on, elements or texts: WebIDL's variadic
    /// `(Element or Text)...`.
    Slottables,
    /// The arguments from one on, nodes or strings: WebIDL's variadic
    /// `(Node or DOMString)...`.
    NodesOrStrings,
    /// Names and values (an argument): WebIDL's `(sequence<sequence<
    /// USVString>> or record<USVString, USVString> or USVString)`,
    /// `URLSearchParams`' init.
    StringPairs,
    /// A sequence of an interface's objects (a result), by its name.
    Sequence(&'static str),
    /// A sequence of numbers (a result): WebIDL's `FrozenArray<double>`.
    Numbers,
    /// Strings (a result): WebIDL's `FrozenArray<DOMString>`.
    FrozenStrings,
    /// A frozen array of an interface's objects (a result): WebIDL's
    /// `FrozenArray<T>`, by its name.
    FrozenArray(&'static str),
    /// Numbers in order (an argument, or a result: a new array): WebIDL's
    /// `sequence<unrestricted double>`.
    Doubles,
    /// A `Uint8ClampedArray`: an argument, the script's own, its bytes
    /// read in place; a result, the one an `ImageData` holds.
    Uint8ClampedArray,
    /// A union of a `DOMString` and interfaces' objects (an argument or a
    /// result), by their names: WebIDL's `(DOMString or CanvasGradient or
    /// CanvasPattern)`.
    StringOr(&'static [&'static str]),
    /// An interface's object, by the interface's name.
    Node(&'static str),
    NullableNode(&'static str),
    /// An observable array of an interface's objects (WebIDL's
    /// `ObservableArray<T>`, an attribute's type), by its name.
    ObservableArray(&'static str),
    /// A promise of an interface's object (a result), by its name.
    Promise(&'static str),
    /// A nullable union of interfaces' objects (a result), by their names:
    /// WebIDL's `(RadioNodeList or Element)?`; or of an interface's and a
    /// number's (an argument, `(HTMLElement or long)?`).
    NullableUnion(&'static [&'static str]),
    /// A union of interfaces' objects (an argument), by their names:
    /// WebIDL's `(HTMLOptionElement or HTMLOptGroupElement)`.
    Union(&'static [&'static str]),
}

/// WebIDL's overload resolution, as far as the table's overloads need:
/// of `forms` (a method's, or the constructors, in
/// the table's order), those that take `given` arguments (or as many as
/// the longest takes, if more); where several do, the one whose argument at
/// the first position where one takes an interface (or a typed array) and
/// another doesn't fits: `implements(index, name)` says whether the
/// argument there is an object of that interface, and a form that takes
/// something else there is the one for any other value. The index of the
/// form; the `TypeError`'s message when none takes that many
/// (`putImageData(data, 1, 2, 3)`), as Chrome words it.
pub fn resolve_overload(
    forms: &[&Member],
    given: usize,
    mut implements: impl FnMut(usize, &str) -> bool,
) -> Result<usize, String> {
    let most = forms
        .iter()
        .map(|form| form.params.len())
        .max()
        .unwrap_or(0);
    let count = given.min(most);
    let candidates: Vec<usize> = (0..forms.len())
        .filter(|&form| forms[form].required() <= count && count <= forms[form].params.len())
        .collect();
    match candidates.as_slice() {
        [] => {
            let mut arities: Vec<usize> = forms
                .iter()
                .flat_map(|form| form.required()..=form.params.len())
                .collect();
            arities.sort_unstable();
            arities.dedup();
            let arities: Vec<String> = arities.iter().map(usize::to_string).collect();
            Err(format!(
                "Valid arities are: [{}], but {given} arguments provided.",
                arities.join(", ")
            ))
        }
        [only] => Ok(*only),
        several => {
            let interface = |form: usize, index: usize| match forms[form].params[index].ty {
                TypeDesc::Node(name) | TypeDesc::NullableNode(name) => Some(name),
                TypeDesc::Uint8ClampedArray => Some("Uint8ClampedArray"),
                _ => None,
            };
            let index = (0..count)
                .find(|&index| {
                    let first = interface(several[0], index).is_some();
                    several
                        .iter()
                        .any(|&form| interface(form, index).is_some() != first)
                })
                .unwrap_or(0);
            Ok(several
                .iter()
                .copied()
                .find(|&form| interface(form, index).is_some_and(|name| implements(index, name)))
                .or_else(|| {
                    several
                        .iter()
                        .copied()
                        .find(|&form| interface(form, index).is_none())
                })
                .unwrap_or(several[0]))
        }
    }
}

/// The interfaces `resolve_overload` asks `implements` about for `forms`:
/// at each number of arguments where several forms take that many, the
/// interfaces their arguments are at the position it tells them apart by.
pub fn overload_interfaces(forms: &[&Member]) -> Vec<&'static str> {
    let most = forms
        .iter()
        .map(|form| form.params.len())
        .max()
        .unwrap_or(0);
    let mut names = Vec::new();
    for count in 0..=most {
        let several: Vec<&Member> = forms
            .iter()
            .copied()
            .filter(|form| form.required() <= count && count <= form.params.len())
            .collect();
        if several.len() < 2 {
            continue;
        }
        let interface = |form: &Member, index: usize| match form.params[index].ty {
            TypeDesc::Node(name) | TypeDesc::NullableNode(name) => Some(name),
            TypeDesc::Uint8ClampedArray => Some("Uint8ClampedArray"),
            _ => None,
        };
        let index = (0..count)
            .find(|&index| {
                let first = interface(several[0], index).is_some();
                several
                    .iter()
                    .any(|form| interface(form, index).is_some() != first)
            })
            .unwrap_or(0);
        names.extend(several.iter().filter_map(|form| interface(form, index)));
    }
    names.sort_unstable();
    names.dedup();
    names
}

/// Whether `INTERFACES[interface].members[member]` is one of an overload's
/// forms (a method another of its interface's methods has the name of, or
/// one of several constructors), at compile time: an engine takes the
/// address of those forms' calls alone, so that the others' stay inlined
/// where they are called.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "i < members.len() and forms <= members.len(), a slice's length"
)]
pub const fn is_overload(interface: usize, member: usize) -> bool {
    let members = crate::interfaces::INTERFACES[interface].members;
    let this = &members[member];
    let mut forms = 0;
    let mut i = 0;
    while i < members.len() {
        let other = &members[i];
        let same_kind = matches!(
            (this.kind, other.kind),
            (MemberKind::Method, MemberKind::Method)
                | (MemberKind::Constructor, MemberKind::Constructor)
        );
        // A static operation and a regular one of a name are none of each
        // other's forms (`Response.json()`, `response.json()`).
        if same_kind
            && other.is_static == this.is_static
            && eq(other.name.as_bytes(), this.name.as_bytes())
        {
            forms += 1;
        }
        i += 1;
    }
    forms > 1
}

/// The index of the interface named `name` in `INTERFACES`, at compile time.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "i < interfaces.len(), a slice's length"
)]
pub const fn interface_index(name: &str) -> usize {
    let interfaces = crate::interfaces::INTERFACES;
    let mut i = 0;
    while i < interfaces.len() {
        if eq(interfaces[i].name.as_bytes(), name.as_bytes()) {
            return i;
        }
        i += 1;
    }
    panic!("an interface of the table")
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "i < a.len(), a slice's length"
)]
const fn eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// Each interface's parent's index in `INTERFACES` (`usize::MAX` for
/// none): looked up by name once, so that the tables below walk indices.
const PARENTS: [usize; crate::interfaces::INTERFACES.len()] = {
    let interfaces = crate::interfaces::INTERFACES;
    let mut table = [usize::MAX; crate::interfaces::INTERFACES.len()];
    let mut interface = 0;
    while interface < interfaces.len() {
        if let Some(parent) = interfaces[interface].parent {
            table[interface] = interface_index(parent);
        }
        interface += 1;
    }
    table
};

/// Whether `INTERFACES[ancestor]` is `INTERFACES[interface]` or one of its
/// ancestors.
pub const fn is_ancestor_or_self(ancestor: usize, mut interface: usize) -> bool {
    loop {
        if interface == ancestor {
            return true;
        }
        match PARENTS[interface] {
            usize::MAX => return false,
            parent => interface = parent,
        }
    }
}

/// How many kinds of node there are (`NodeKind`'s variants).
const NODE_KINDS: usize = 8;

/// A node kind's bit in `ACCEPTED` (a kind is under `NODE_KINDS`: the
/// shift fits a `u8`).
pub(crate) const fn kind_bit(kind: NodeKind) -> u8 {
    1 << kind as u8
}

/// Each interface's node kinds, a bit each (`kind_bit`): its own and its
/// descendants', worked out at compile time, so that checking a node's
/// interface (a node argument, a guest's `this`) is a load and a mask, as
/// Blink's `WrapperTypeInfo` is a pointer to compare along its parents.
pub(crate) const ACCEPTED: [u8; crate::interfaces::INTERFACES.len()] = {
    let interfaces = crate::interfaces::INTERFACES;
    let mut table = [0; crate::interfaces::INTERFACES.len()];
    let mut ancestor = 0;
    while ancestor < interfaces.len() {
        let mut interface = 0;
        while interface < interfaces.len() {
            if is_ancestor_or_self(ancestor, interface) {
                // An element interface's are some elements: its names
                // checked after (`BY_NAME`).
                if interfaces[interface].element.is_some() {
                    table[ancestor] |= kind_bit(NodeKind::Element);
                }
                let kinds = interfaces[interface].kinds;
                let mut at = 0;
                while at < kinds.len() {
                    table[ancestor] |= kind_bit(kinds[at]);
                    at += 1;
                }
            }
            interface += 1;
        }
        ancestor += 1;
    }
    table
};

/// Each node kind's most derived interface, by its discriminant: a node's
/// wrapper's, worked out at compile time.
pub(crate) const NODE_INTERFACES: [usize; NODE_KINDS] = {
    let interfaces = crate::interfaces::INTERFACES;
    let mut table = [usize::MAX; NODE_KINDS];
    let mut interface = 0;
    while interface < interfaces.len() {
        let kinds = interfaces[interface].kinds;
        let mut at = 0;
        while at < kinds.len() {
            table[kinds[at] as usize] = interface;
            at += 1;
        }
        interface += 1;
    }
    let mut kind = 0;
    while kind < NODE_KINDS {
        assert!(
            table[kind] != usize::MAX,
            "each kind of node is some interface's"
        );
        kind += 1;
    }
    table
};

/// `HTMLElement`'s index: the interface of an HTML element whose name
/// HTML defines with no interface of its own, and of a custom element's.
pub const HTML_ELEMENT: usize = interface_index("HTMLElement");

/// `HTMLUnknownElement`'s index: an HTML element whose name HTML doesn't
/// define (nor a valid custom element name), or a failed custom element.
pub const HTML_UNKNOWN_ELEMENT: usize = interface_index("HTMLUnknownElement");

/// The elements HTML defines, obsolete ones included, but for those whose
/// interface HTML makes `HTMLUnknownElement` (`<applet>`, `<blink>`...).
/// Each has an interface of its own or `HTMLElement`.
const HTML_ELEMENTS: &[&str] = &[
    "a",
    "abbr",
    "acronym",
    "address",
    "area",
    "article",
    "aside",
    "audio",
    "b",
    "base",
    "basefont",
    "bdi",
    "bdo",
    "big",
    "blockquote",
    "body",
    "br",
    "button",
    "canvas",
    "caption",
    "center",
    "cite",
    "code",
    "col",
    "colgroup",
    "data",
    "datalist",
    "dd",
    "del",
    "details",
    "dfn",
    "dialog",
    "dir",
    "div",
    "dl",
    "dt",
    "em",
    "embed",
    "fieldset",
    "figcaption",
    "figure",
    "font",
    "footer",
    "form",
    "frame",
    "frameset",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hgroup",
    "hr",
    "html",
    "i",
    "iframe",
    "img",
    "input",
    "ins",
    "kbd",
    "label",
    "legend",
    "li",
    "link",
    "listing",
    "main",
    "map",
    "mark",
    "marquee",
    "menu",
    "meta",
    "meter",
    "nav",
    "nobr",
    "noembed",
    "noframes",
    "noscript",
    "object",
    "ol",
    "optgroup",
    "option",
    "output",
    "p",
    "param",
    "picture",
    "plaintext",
    "pre",
    "progress",
    "q",
    "rb",
    "rp",
    "rt",
    "rtc",
    "ruby",
    "s",
    "samp",
    "script",
    "search",
    "section",
    "select",
    "slot",
    "small",
    "source",
    "span",
    "strike",
    "strong",
    "style",
    "sub",
    "summary",
    "sup",
    "table",
    "tbody",
    "td",
    "template",
    "textarea",
    "tfoot",
    "th",
    "thead",
    "time",
    "title",
    "tr",
    "track",
    "tt",
    "u",
    "ul",
    "var",
    "video",
    "wbr",
    "xmp",
];

/// Each HTML local name's interface, by the name's fixed number
/// (`dom::names::fixed`): `HTMLElement`'s for a name HTML defines with
/// none of its own, `HTMLUnknownElement`'s for one it doesn't define. Two
/// bytes each (the table's interfaces are fewer than 65536, which the
/// assertion below checks), so that the table, by every fixed name, takes
/// a few KiB.
#[expect(
    clippy::cast_possible_truncation,
    reason = "fewer than 65536 interfaces (asserted): one fits a u16"
)]
pub(crate) static HTML_TAGS: [u16; dom::names::FIXED_COUNT] = {
    let interfaces = crate::interfaces::INTERFACES;
    assert!(
        interfaces.len() <= u16::MAX as usize,
        "an interface's index fits a u16"
    );
    let mut table = [HTML_UNKNOWN_ELEMENT as u16; dom::names::FIXED_COUNT];
    let mut known = 0;
    while known < HTML_ELEMENTS.len() {
        // An obsolete name the fixed ones lack is an `HTMLUnknownElement`.
        if let Some(id) = dom::names::fixed(HTML_ELEMENTS[known]) {
            table[id as usize] = HTML_ELEMENT as u16;
        }
        known += 1;
    }
    let mut interface = 0;
    while interface < interfaces.len() {
        if let Some(ElementDesc { svg: false, locals }) = interfaces[interface].element {
            let mut at = 0;
            while at < locals.len() {
                match dom::names::fixed(locals[at]) {
                    Some(id) => table[id as usize] = interface as u16,
                    None => {
                        panic!("an element interface's local name is one of the names' fixed ones")
                    }
                }
                at += 1;
            }
        }
        interface += 1;
    }
    table
};

/// `SVGElement`'s index: an SVG element whose name has no interface of its
/// own.
pub(crate) const SVG_ELEMENT: usize = interface_index("SVGElement");

/// Each SVG local name's interface, by the name's fixed number, as
/// `HTML_TAGS`: `SVGElement`'s for a name with none of its own.
#[expect(
    clippy::cast_possible_truncation,
    reason = "fewer than 65536 interfaces (`HTML_TAGS` asserts it): one fits a u16"
)]
pub(crate) static SVG_TAGS: [u16; dom::names::FIXED_COUNT] = {
    let interfaces = crate::interfaces::INTERFACES;
    let mut table = [SVG_ELEMENT as u16; dom::names::FIXED_COUNT];
    let mut interface = 0;
    while interface < interfaces.len() {
        if let Some(ElementDesc { svg: true, locals }) = interfaces[interface].element {
            let mut at = 0;
            while at < locals.len() {
                match dom::names::fixed(locals[at]) {
                    Some(id) => table[id as usize] = interface as u16,
                    None => {
                        panic!("an element interface's local name is one of the names' fixed ones")
                    }
                }
                at += 1;
            }
        }
        interface += 1;
    }
    table
};

/// How many words `DESCENDANTS` takes an interface's bits in: as many as
/// the table's interfaces need.
pub(crate) const DESCENDANT_WORDS: usize = crate::interfaces::INTERFACES.len().div_ceil(128);

/// Each interface's descendants and itself, a bit each by index (in
/// `DESCENDANT_WORDS` words), for the interfaces whose objects a node's
/// kind doesn't tell (`BY_NAME`).
pub(crate) const DESCENDANTS: [[u128; DESCENDANT_WORDS]; crate::interfaces::INTERFACES.len()] = {
    let interfaces = crate::interfaces::INTERFACES;
    let mut table = [[0; DESCENDANT_WORDS]; crate::interfaces::INTERFACES.len()];
    let mut ancestor = 0;
    while ancestor < interfaces.len() {
        let mut interface = 0;
        while interface < interfaces.len() {
            if is_ancestor_or_self(ancestor, interface) {
                table[ancestor][interface / 128] |= 1 << (interface % 128);
            }
            interface += 1;
        }
        ancestor += 1;
    }
    table
};

/// Whether an interface's objects need their names checked as well as
/// their kind: an element interface's, and its descendants'.
pub(crate) const BY_NAME: [bool; crate::interfaces::INTERFACES.len()] = {
    let interfaces = crate::interfaces::INTERFACES;
    let mut table = [false; crate::interfaces::INTERFACES.len()];
    let mut interface = 0;
    while interface < interfaces.len() {
        let mut ancestor = 0;
        while ancestor < interfaces.len() {
            if interfaces[ancestor].element.is_some() && is_ancestor_or_self(ancestor, interface) {
                table[interface] = true;
            }
            ancestor += 1;
        }
        interface += 1;
    }
    table
};
