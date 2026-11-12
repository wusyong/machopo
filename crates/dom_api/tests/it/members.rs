//! The dictionaries' member names are fixed names of the DOM's table
//! (`crates/dom/dictionary_members.txt`): a member whose name isn't listed
//! doesn't compile (`MemberName::of`); this checks the other way, that the
//! list has nothing no dictionary names, and is in order.

use std::collections::BTreeSet;
use std::path::Path;

/// The member names of every `dictionary` block of the crate's sources.
fn declared() -> BTreeSet<String> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut names = BTreeSet::new();
    for entry in std::fs::read_dir(src).expect("src") {
        let path = entry.expect("an entry").path();
        if path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("a source file");
        let mut inside = false;
        for line in source.lines() {
            let line = line.split("//").next().unwrap_or("").trim();
            if !inside {
                inside = line.starts_with("dictionary ") && line.ends_with('{');
                continue;
            }
            if line == "}" {
                inside = false;
                continue;
            }
            if let Some((name, _)) = line.split_once(':') {
                let name = name.trim().trim_start_matches("r#");
                if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    names.insert(name.to_owned());
                }
            }
        }
    }
    names
}

#[test]
fn the_listed_member_names_are_the_dictionaries() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../dom/dictionary_members.txt");
    let list = std::fs::read_to_string(path).expect("dictionary_members.txt");
    let listed: Vec<&str> = list
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    let declared = declared();
    assert!(declared.len() > 100, "the dictionaries' members were found");
    assert!(
        declared.contains("bubbles") && declared.contains("type") && declared.contains("clientX")
    );
    let unused: Vec<&&str> = listed
        .iter()
        .filter(|name| !declared.contains(**name))
        .collect();
    assert!(unused.is_empty(), "listed, no dictionary's: {unused:?}");
    let missing: Vec<&String> = declared
        .iter()
        .filter(|name| !listed.contains(&name.as_str()))
        .collect();
    assert!(missing.is_empty(), "not listed: {missing:?}");
    assert!(listed.is_sorted(), "in order");
    assert!(
        listed.iter().all(|name| dom::names::fixed(name).is_some()),
        "each a fixed name"
    );
}
