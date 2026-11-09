//! The fixed names (`src/names.rs`): every thread's table has them at fixed
//! numbers, known at compile time, as Blink's `html_names`, `svg_names` and
//! `mathml_names` are generated. The DOM's own (namespaces, `nodeName`s)
//! first, at the numbers its constants give them; then html5ever's static
//! atoms (`fixed_names.txt`: web_atoms' `local_names.txt`, MIT or
//! Apache-2.0), the HTML, SVG and MathML elements' and attributes' local
//! names, then each of those local names uppercased (an HTML element's
//! `tagName` in an HTML document: Blink's `QualifiedName` keeps its
//! uppercased local name). Generates the names by number, each local name's
//! uppercased one's number, a perfect hash from a name to its
//! number (CHD, as `phf`'s, with a cheaper hash: `src/name_hash.rs`), and a
//! constant for each (`names::local::DIV`). Then Blink's event types
//! (`event_type_names.txt`), those not there already, with no uppercased
//! ones, a constant each in `names::event` (`event::CLICK`, Blink's
//! `event_type_names::kClick`). Then the event handlers' attribute names
//! (`event_handlers.txt`) not there already, and by number, which event
//! type each handles and in which set (`names::handler_attribute`).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

include!("src/name_hash.rs");

/// The perfect hash's buckets and slots (`name_hash_parts` takes the top 10
/// bits for a bucket): about two names a bucket, and slots for nearly twice
/// the names (8 KiB), so that a seed is found at once.
const BUCKETS: usize = 1024;
const SLOTS: usize = 4096;

/// The DOM's own, at the numbers `names.rs`'s constants give them.
const OWN: [&str; 17] = [
    "",
    "http://www.w3.org/1999/xhtml",
    "http://www.w3.org/2000/svg",
    "http://www.w3.org/1998/Math/MathML",
    "http://www.w3.org/1999/xlink",
    "http://www.w3.org/XML/1998/namespace",
    "http://www.w3.org/2000/xmlns/",
    "#document",
    "#text",
    "id",
    "class",
    "#comment",
    "#document-fragment",
    "template",
    "meta",
    "name",
    "content",
];

/// The UA's own attributes, after the fixed list (no number before them
/// moves): a UA shadow tree's element's shadow pseudo id (Blink's
/// `pseudo`), a range's thumb's ratio, a menu list's options' labels.
const USER_AGENT: [&str; 3] = ["pseudo", "ratio", "labels"];

/// The constants of the attributes whose names would be an SVG element's
/// (Blink's `svg_names::kClipPathAttr` beside `kClipPathTag`).
const ATTRIBUTES: [(&str, &str); 2] = [
    ("clip-path", "CLIP_PATH_ATTR"),
    ("solid-color", "SOLID_COLOR_ATTR"),
];

/// A local name's constant: uppercased, an underscore for what isn't a
/// letter or a digit and before an uppercase letter (SVG's `solidColor` is
/// `SOLID_COLOR`, `solidcolor` `SOLIDCOLOR`), and before a leading digit
/// (`*` is `STAR`).
fn constant(name: &str) -> String {
    if name == "*" {
        return "STAR".to_owned();
    }
    if let Some((_, constant)) = ATTRIBUTES.iter().find(|(attribute, _)| *attribute == name) {
        return (*constant).to_owned();
    }
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_uppercase() {
            out.push('_');
        }
        out.push(if c.is_ascii_alphanumeric() {
            c.to_ascii_uppercase()
        } else {
            '_'
        });
    }
    if out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

/// An event type's constant: uppercased, an underscore between words of
/// its camel case (`DOMContentLoaded` is `DOM_CONTENT_LOADED`,
/// `webkitAnimationEnd` `WEBKIT_ANIMATION_END`) and for what isn't a
/// letter or a digit.
fn event_constant(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::new();
    for (at, &c) in chars.iter().enumerate() {
        let before = at.checked_sub(1).map(|before| chars[before]);
        let after = at.checked_add(1).and_then(|after| chars.get(after));
        // A word starts at an uppercase letter after a lowercase one or a
        // digit, or at the last of a run of capitals before a lowercase.
        let starts = c.is_ascii_uppercase()
            && before.is_some_and(|before| {
                before.is_ascii_lowercase()
                    || before.is_ascii_digit()
                    || (before.is_ascii_uppercase() && after.is_some_and(char::is_ascii_lowercase))
            });
        if starts {
            out.push('_');
        }
        out.push(if c.is_ascii_alphanumeric() {
            c.to_ascii_uppercase()
        } else {
            '_'
        });
    }
    out
}

fn main() {
    println!("cargo::rerun-if-changed=fixed_names.txt");
    println!("cargo::rerun-if-changed=event_type_names.txt");
    println!("cargo::rerun-if-changed=event_handlers.txt");
    let list = std::fs::read_to_string("fixed_names.txt").expect("fixed_names.txt");
    let mut names: Vec<&str> = OWN.to_vec();
    for name in list
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .chain(USER_AGENT)
    {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    // The local names: not the namespaces, the empty name or `nodeName`s.
    let is_local = |name: &str| !name.is_empty() && !name.starts_with('#') && !name.contains("://");
    let local_count = names.len();
    let mut uppercased: Vec<String> = Vec::new();
    for name in names.iter().filter(|name| is_local(name)) {
        let upper = name.to_ascii_uppercase();
        if !names.contains(&upper.as_str()) && !uppercased.contains(&upper) {
            uppercased.push(upper);
        }
    }
    names.extend(uppercased.iter().map(String::as_str));
    let event_list = std::fs::read_to_string("event_type_names.txt").expect("event_type_names.txt");
    let event_types: Vec<&str> = event_list
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    for &event_type in &event_types {
        if !names.contains(&event_type) {
            names.push(event_type);
        }
    }
    let handler_list = std::fs::read_to_string("event_handlers.txt").expect("event_handlers.txt");
    let handlers: Vec<(&str, &str, &str)> = handler_list
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let mut words = line.split_whitespace();
            let (Some(set), Some(attribute), Some(event_type), None) =
                (words.next(), words.next(), words.next(), words.next())
            else {
                panic!("not `set attribute type`: {line}");
            };
            assert!(
                event_types.contains(&event_type),
                "{event_type} isn't an event type of the list"
            );
            (set, attribute, event_type)
        })
        .collect();
    for &(_, attribute, _) in &handlers {
        if !names.contains(&attribute) {
            names.push(attribute);
        }
    }
    let numbers: std::collections::HashMap<&str, usize> = names
        .iter()
        .enumerate()
        .map(|(number, name)| (*name, number))
        .collect();
    let uppercase: Vec<usize> = names
        .iter()
        .enumerate()
        .map(
            |(number, name)| match number < local_count && is_local(name) {
                true => numbers[name.to_ascii_uppercase().as_str()],
                false => number,
            },
        )
        .collect();
    assert!(names.len() < SLOTS && SLOTS <= usize::from(u16::MAX));
    let (seed, displacements, slots) = (0u64..)
        .find_map(|seed| perfect_hash(&names, seed).map(|(d, s)| (seed, d, s)))
        .expect("a seed");
    let mut constants = BTreeMap::new();
    // The uppercased ones have none: `DIV` would be `div`'s.
    let locals = names
        .iter()
        .enumerate()
        .take(local_count)
        .filter(|(_, name)| is_local(name));
    for (number, name) in locals {
        let constant = constant(name);
        if let Some(other) = constants.insert(constant.clone(), number) {
            panic!("{name}'s constant {constant} is {}'s too", names[other]);
        }
    }
    let mut events = BTreeMap::new();
    for &event_type in &event_types {
        let constant = event_constant(event_type);
        let number = numbers_of(&names, event_type);
        if let Some(other) = events.insert(constant.clone(), number) {
            panic!(
                "{event_type}'s constant {constant} is {}'s too",
                names[other]
            );
        }
    }
    // By number: 0, or the event type an attribute's handler handles, with
    // its sets' bits (`names::handler_attribute`).
    let mut handler_attributes = vec![0u16; names.len()];
    for &(set, attribute, event_type) in &handlers {
        let event_type = numbers_of(&names, event_type);
        assert!(event_type < 1 << 13, "an event type's number in 13 bits");
        let bit = match set {
            "global" => 1 << 13,
            "window" => 1 << 14,
            "reflecting" => 1 << 15,
            other => panic!("no set {other}"),
        };
        let entry = &mut handler_attributes[numbers_of(&names, attribute)];
        assert!(
            *entry == 0 || usize::from(*entry & 0x1fff) == event_type,
            "{attribute}'s type twice"
        );
        *entry |= u16::try_from(event_type).expect("13 bits") | bit;
    }
    // By number, what an HTML element of the local name is to forms
    // (`form::kind`'s bits): listed 1, labelable 2, a form 4, an input 8, a
    // fieldset 16, one a `disabled` attribute disables 64, a textarea 128;
    // and what an attribute of the name is: one a form element's state
    // (or an `<img>`'s image data, a script element's flags) follows 32.
    let mut form_kinds = vec![0u8; names.len()];
    for (name, kind) in [
        ("button", 3 | 64),
        ("fieldset", 1 | 16 | 64),
        ("input", 3 | 8 | 64),
        ("object", 1),
        ("output", 3),
        ("select", 3 | 64),
        ("textarea", 3 | 64 | 128),
        ("meter", 2),
        ("progress", 2),
        ("form", 4 | 32),
        ("type", 32),
        ("checked", 32),
        ("name", 32),
        ("disabled", 32),
        ("value", 32),
        ("placeholder", 32),
        ("min", 32),
        ("max", 32),
        ("step", 32),
        ("multiple", 32),
        ("readonly", 32),
        ("selected", 32),
        ("size", 32),
        ("label", 32),
        // An `<img>`'s and a `<source>`'s (`image`'s relevant mutations);
        // as an element's name, a `<picture>`, whose making is looked at.
        ("picture", 32),
        ("media", 32),
        ("src", 32),
        ("srcset", 32),
        ("sizes", 32),
        ("crossorigin", 32),
        ("referrerpolicy", 32),
        ("loading", 32),
        // A script element's `async` (and `src`, above); a `<link>`'s `rel`.
        ("async", 32),
        ("rel", 32),
        // A `<canvas>`'s bitmap size.
        ("width", 32),
        ("height", 32),
    ] {
        form_kinds[numbers_of(&names, name)] = kind;
    }
    let mut out = String::new();
    writeln!(out, "/// How many names every table has at fixed numbers.").unwrap();
    writeln!(out, "pub const FIXED_COUNT: usize = {};", names.len()).unwrap();
    writeln!(out, "/// The fixed names, by number.").unwrap();
    writeln!(out, "static FIXED: [&str; FIXED_COUNT] = {names:?};").unwrap();
    writeln!(
        out,
        "/// Each fixed name's uppercased one's number (its own if it isn't a"
    )
    .unwrap();
    writeln!(out, "/// local name).").unwrap();
    writeln!(out, "static UPPERCASE: [u16; FIXED_COUNT] = {uppercase:?};").unwrap();
    writeln!(
        out,
        "/// The perfect hash's seed, its buckets' displacements, and by slot"
    )
    .unwrap();
    writeln!(
        out,
        "/// the number of the name there (0, the empty name, in an empty one)."
    )
    .unwrap();
    writeln!(out, "const SEED: u64 = {seed};").unwrap();
    writeln!(
        out,
        "static DISPLACEMENTS: [(u16, u16); {BUCKETS}] = {displacements:?};"
    )
    .unwrap();
    writeln!(out, "static SLOTS: [u16; {SLOTS}] = {slots:?};").unwrap();
    writeln!(out, "const SLOT_MASK: u32 = {};", SLOTS - 1).unwrap();
    writeln!(
        out,
        "/// The fixed local names' numbers (html5ever's `local_name!`)."
    )
    .unwrap();
    writeln!(out, "pub mod local {{").unwrap();
    writeln!(out, "    use super::NameId;").unwrap();
    for (constant, number) in &constants {
        writeln!(out, "    pub const {constant}: NameId = {number};").unwrap();
    }
    writeln!(out, "}}").unwrap();
    writeln!(
        out,
        "/// The fixed event types' numbers (Blink's `event_type_names`)."
    )
    .unwrap();
    writeln!(
        out,
        "/// By number, the event type an attribute's handler handles and its sets"
    )
    .unwrap();
    writeln!(out, "/// (`names::handler_attribute`); 0 for others.").unwrap();
    writeln!(
        out,
        "static HANDLER_ATTRIBUTES: [u16; FIXED_COUNT] = {handler_attributes:?};"
    )
    .unwrap();
    writeln!(
        out,
        "/// By number, what an HTML element of the local name is to forms"
    )
    .unwrap();
    writeln!(out, "/// (`form::kind`); 0 for others.").unwrap();
    writeln!(
        out,
        "static FORM_KINDS: [u8; FIXED_COUNT] = {form_kinds:?};"
    )
    .unwrap();
    for (set, constant) in [
        ("global", "GLOBAL_EVENT_HANDLERS"),
        ("window", "WINDOW_EVENT_HANDLERS"),
    ] {
        let list: Vec<(usize, usize)> = handlers
            .iter()
            .filter(|(of, ..)| *of == set)
            .map(|&(_, attribute, event_type)| {
                (
                    numbers_of(&names, attribute),
                    numbers_of(&names, event_type),
                )
            })
            .collect();
        writeln!(
            out,
            "/// The {set} event handlers: their attributes' names and their types."
        )
        .unwrap();
        writeln!(
            out,
            "pub const {constant}: &[(NameId, NameId)] = &{list:?};"
        )
        .unwrap();
    }
    let reflecting: Vec<usize> = handlers
        .iter()
        .filter(|(set, ..)| *set == "reflecting")
        .map(|&(_, _, event_type)| numbers_of(&names, event_type))
        .collect();
    writeln!(
        out,
        "/// The types of the global event handlers a `<body>` or `<frameset>` has"
    )
    .unwrap();
    writeln!(
        out,
        "/// for its window (HTML's \"Window-reflecting body element\" ones)."
    )
    .unwrap();
    writeln!(
        out,
        "pub const WINDOW_REFLECTING: &[NameId] = &{reflecting:?};"
    )
    .unwrap();
    writeln!(out, "pub mod event {{").unwrap();
    writeln!(out, "    use super::NameId;").unwrap();
    for (constant, number) in &events {
        writeln!(out, "    pub const {constant}: NameId = {number};").unwrap();
    }
    writeln!(out, "}}").unwrap();
    let path = Path::new(&std::env::var("OUT_DIR").expect("OUT_DIR")).join("fixed_names.rs");
    std::fs::write(path, out).expect("the generated names");
}

/// `name`'s number among `names`.
fn numbers_of(names: &[&str], name: &str) -> usize {
    names
        .iter()
        .position(|&other| other == name)
        .expect("a name of the list")
}

/// A perfect hash's displacements by bucket, and each slot's name's number.
type PerfectHash = (Vec<(u16, u16)>, Vec<u16>);

/// A perfect hash of `names` with `seed` (CHD: the buckets with the most
/// names first, each given the first displacements that put its names in
/// free slots), if one is found: the displacements by bucket, and each
/// slot's name's number.
fn perfect_hash(names: &[&str], seed: u64) -> Option<PerfectHash> {
    let parts: Vec<_> = names
        .iter()
        .map(|name| name_hash_parts(name_hash(name.as_bytes(), seed)))
        .collect();
    let mut buckets = vec![Vec::new(); BUCKETS];
    for (number, &(bucket, _, _)) in parts.iter().enumerate().skip(1) {
        buckets[bucket].push(number);
    }
    let mut order: Vec<usize> = (0..BUCKETS).collect();
    order.sort_by_key(|&bucket| std::cmp::Reverse(buckets[bucket].len()));
    let mask = u32::try_from(SLOTS - 1).unwrap();
    let mut slots = vec![0u16; SLOTS];
    let mut displacements = vec![(0, 0); BUCKETS];
    let mut taken = Vec::new();
    for bucket in order {
        if buckets[bucket].is_empty() {
            break;
        }
        let found = (0..u16::MAX)
            .flat_map(|d1| (0..64).map(move |d2| (d1, d2)))
            .take(1 << 16)
            .find(|&displacement| {
                taken.clear();
                buckets[bucket].iter().all(|&number| {
                    let (_, f1, f2) = parts[number];
                    let slot = name_hash_slot(f1, f2, displacement, mask);
                    let free = slots[slot] == 0 && !taken.contains(&slot);
                    taken.push(slot);
                    free
                })
            })?;
        displacements[bucket] = found;
        for &number in &buckets[bucket] {
            let (_, f1, f2) = parts[number];
            slots[name_hash_slot(f1, f2, found, mask)] = u16::try_from(number).unwrap();
        }
    }
    Some((displacements, slots))
}
