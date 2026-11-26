//! Every family of the system's, registered into a collection with no system
//! fonts the way the prototype does (a file of one face under the system's
//! name and attributes, a collection as it is), and compared with the
//! system's own.
use std::collections::HashMap;

use fontique::{Collection, CollectionOptions, FontInfoOverride, SourceCache};

fn key(font: &fontique::FontInfo) -> (u32, String, String) {
    (
        font.weight().value().round() as u32,
        format!("{:?}", font.style()),
        format!("{:.3}", font.width().ratio()),
    )
}

fn main() {
    let mut system = Collection::new(CollectionOptions { shared: false, system_fonts: true });
    let mut cache = SourceCache::default();
    let mut ours = Collection::new(CollectionOptions { shared: false, system_fonts: false });
    let mut files: HashMap<(u64, Option<String>), Vec<(fontique::FamilyId, Vec<fontique::FontInfo>)>> = HashMap::new();
    let mut again = 0;
    let mut seen_single: std::collections::HashSet<u64> = Default::default();
    let names: Vec<String> = system.family_names().map(str::to_owned).collect();
    let (mut families, mut ttc_faces, mut single_faces) = (0, 0, 0);
    let mut problems: Vec<String> = Vec::new();
    for name in &names {
        let Some(family) = system.family_by_name(name) else { continue };
        if family.name() != name {
            continue; // an alias of a family seen under its own name
        }
        families += 1;
        let mut ids = Vec::new();
        for font in family.fonts() {
            let source = font.source().id().to_u64();
            let blob = cache.get(font.source());
            let single = blob.as_ref().is_some_and(|blob| !matches!(
                read_fonts::FileRef::new(blob.as_ref()),
                Ok(read_fonts::FileRef::Collection(_))
            ));
            let id = (source, single.then(|| family.name().to_owned()));
            if !files.contains_key(&id) {
                if single && !seen_single.insert(source) {
                    again += 1;
                }
                let registered = match blob {
                    Some(blob) => {
                        let over = single.then(|| FontInfoOverride {
                            family_name: Some(family.name()),
                            width: Some(font.width()),
                            style: Some(font.style()),
                            weight: Some(font.weight()),
                            axes: None,
                        });
                        ours.register_fonts(blob, over)
                    }
                    None => Vec::new(),
                };
                files.insert(id.clone(), registered);
            }
            if single { single_faces += 1 } else { ttc_faces += 1 }
            match files[&id].iter().find(|(_, infos)| infos.iter().any(|i| i.index() == font.index())) {
                Some((id, infos)) => {
                    ids.push(*id);
                    let theirs = infos.iter().find(|i| i.index() == font.index()).unwrap();
                    if key(theirs) != key(font) {
                        problems.push(format!("{name}: face {} is {:?} by the file, {:?} by the system", font.index(), key(theirs), key(font)));
                    }
                }
                None => problems.push(format!("{name}: a face of it isn't in its file as registered")),
            }
        }
        ids.dedup();
        ids.sort_by_key(|id| id.to_u64());
        ids.dedup();
        if ids.len() > 1 {
            let split: Vec<_> = ids.iter().filter_map(|id| ours.family_name(*id).map(str::to_owned)).collect();
            problems.push(format!("{name}: its faces are {} families: {split:?}", ids.len()));
        }
        match (ids.first(), ours.family_id(name)) {
            (Some(found), Some(by_name)) if *found == by_name => {}
            (Some(found), by_name) => {
                let registered = ours.family_name(*found).map(str::to_owned);
                let finds = by_name.and_then(|id| ours.family_name(id).map(str::to_owned));
                problems.push(format!(
                    "{name}: registered as {registered:?}, and the name finds {finds:?}"
                ));
            }
            (None, _) => problems.push(format!("{name}: nothing registered")),
        }
    }
    println!("{} names, {families} families; faces: {single_faces} in files of one, {ttc_faces} in collections", names.len());
    println!("{again} files of one face registered again for another family of theirs");
    let mut aliases = Vec::new();
    for name in &names {
        let Some(family) = system.family_by_name(name) else { continue };
        if family.name() != name && ours.family_id(name).is_none() {
            aliases.push(format!("{name} (is {})", family.name()));
        }
    }
    println!("{} other names of a family that the registered collection doesn't know: {:?}", aliases.len(), &aliases[..aliases.len().min(6)]);
    println!("{} problems", problems.len());
    for problem in problems.iter().take(40) {
        println!("  {problem}");
    }
}
