//! The system's fonts as one process finds them and another uses them
//! (Chromium's `FontDataService`): the browser has the system's fonts
//! ([`SystemFontManager`], what `FontDataServiceImpl` answers from), and a
//! renderer's text asks it for what it needs as it needs it
//! ([`FontDataService`]), opening no font file and asking the system
//! nothing itself.
//!
//! What crosses: at the start, every name a family goes by and the families
//! each generic family stands for ([`FontNames`]); then, the first time it
//! is needed, a family's faces ([`FontFamilyFaces`]), the families a
//! script falls back to, and the bytes of the files the faces are in, each
//! file once. Families and files are numbers the browser gave: the
//! renderer is never told a path.
//!
//! The renderer's font collection has none of the system's fonts of its
//! own: the faces of each family it is given are registered in it, a
//! generic family's families set when the generic is first used, and a
//! script's fallback set before a text that has a character of it is
//! shaped (parley asks the collection for it only once shaping has
//! begun).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use parley::FontContext;
use parley::fontique::{
    Blob, Collection, FallbackKey, FamilyId, FontInfoOverride, FontStyle, FontWeight, FontWidth,
    Script, SourceKind,
};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::font_families::FamilyLookup;
use crate::text::SystemFonts;

/// What a renderer is given of the system's fonts at the start: each name
/// a family goes by (its other names too, as the system has them, case as
/// it is) and the family's number, and the families each generic family
/// stands for, in [`parley::GenericFamily::all`]'s order. A family's number
/// is below `families`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FontNames {
    pub names: Vec<(String, u32)>,
    pub families: u32,
    pub generics: Vec<Vec<u32>>,
}

/// A face's slant, as the system has it (fontique's `FontStyle`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FaceStyle {
    Normal,
    Italic,
    /// An angle in degrees, if the face has one.
    Oblique(Option<f32>),
}

/// One face of a family: which file it is in (a number the browser gave
/// the file), which of the file's faces it is, and what the system says
/// of it.
#[derive(Debug, Clone, PartialEq)]
pub struct FontFace {
    pub file: u32,
    pub index: u32,
    pub weight: f32,
    pub style: FaceStyle,
    /// As a ratio of the normal width.
    pub width: f32,
}

/// A family's faces, and its name as the system has it (the first of its
/// names).
#[derive(Debug, Clone, PartialEq)]
pub struct FontFamilyFaces {
    pub name: String,
    pub faces: Vec<FontFace>,
}

/// How many files one answer gives at most: a message's handles.
pub const FONT_FILES_PER_CALL: usize = 16;

/// A font file's bytes, as the renderer reads them (a mapping of the file
/// the browser opened).
pub type FontFileBytes = Arc<dyn AsRef<[u8]> + Send + Sync>;

/// How a renderer asks the browser of the system's fonts (Chromium's
/// `font_data_service::mojom::FontDataService`): each call waits for its
/// answer. Asked only for numbers [`FontNames`] or an earlier answer gave.
pub trait FontDataService {
    /// The faces of family `family`; none if it has none any more, or
    /// there is no answer.
    fn family(&mut self, family: u32) -> Option<FontFamilyFaces>;
    /// The families text of `script` (an ISO 15924 tag) falls back to.
    fn fallback(&mut self, script: [u8; 4]) -> Vec<u32>;
    /// The bytes of each of `files` (at most [`FONT_FILES_PER_CALL`]), in
    /// order: none for one that couldn't be opened.
    fn files(&mut self, files: &[u32]) -> Vec<Option<FontFileBytes>>;
}

/// The system's fonts as the browser answers of them (what Chromium's
/// `FontDataServiceImpl` asks its `SkFontMgr`): the same lookups a
/// renderer's own collection of the system's fonts would make, with the
/// families and the files each a number.
pub struct SystemFontManager {
    fonts: FontContext,
    /// Each family's id in `fonts`, by its number.
    families: Vec<FamilyId>,
    numbers: FxHashMap<FamilyId, u32>,
    names: FontNames,
    /// Each file a face was answered with, by its number.
    files: Vec<PathBuf>,
    file_numbers: FxHashMap<PathBuf, u32>,
}

impl SystemFontManager {
    pub fn new(fonts: SystemFonts) -> Self {
        Self::with_fonts(fonts.into_context())
    }

    /// The fonts in `directories` as though they were the system's, each
    /// generic family of `generics` (by its CSS keyword) the family named
    /// with it, for tests.
    pub fn for_testing(directories: &[&Path], generics: &[(&str, &str)]) -> Self {
        let mut collection = Collection::new(parley::fontique::CollectionOptions {
            shared: false,
            system_fonts: false,
        });
        collection.load_fonts_from_paths(directories);
        for &(generic, name) in generics {
            if let (Some(generic), Some(family)) = (
                parley::GenericFamily::parse(generic),
                collection.family_id(name),
            ) {
                collection.set_generic_families(generic, std::iter::once(family));
            }
        }
        Self::with_fonts(FontContext {
            collection,
            source_cache: parley::fontique::SourceCache::default(),
        })
    }

    fn with_fonts(mut fonts: FontContext) -> Self {
        let collection = &mut fonts.collection;
        let families: Vec<FamilyId> = collection.family_ids().collect();
        let numbers: FxHashMap<FamilyId, u32> = families
            .iter()
            .zip(0_u32..)
            .map(|(&id, number)| (id, number))
            .collect();
        let mut names: Vec<String> = collection.family_names().map(str::to_owned).collect();
        names.sort_unstable();
        let names = names
            .into_iter()
            .filter_map(|name| {
                let number = *numbers.get(&collection.family_id(&name)?)?;
                Some((name, number))
            })
            .collect();
        let generics = parley::GenericFamily::all()
            .iter()
            .map(|&generic| {
                collection
                    .generic_families(generic)
                    .filter_map(|id| numbers.get(&id).copied())
                    .collect()
            })
            .collect();
        let names = FontNames {
            names,
            families: u32::try_from(families.len()).unwrap_or(u32::MAX),
            generics,
        };
        Self {
            fonts,
            families,
            numbers,
            names,
            files: Vec::new(),
            file_numbers: FxHashMap::default(),
        }
    }

    /// What every renderer is given at the start.
    pub fn names(&self) -> &FontNames {
        &self.names
    }

    /// Family `family`'s faces, each in a file of the system's (each file
    /// numbered the first time it is answered with); none if it has no
    /// such face, or there is no such family.
    pub fn family(&mut self, family: u32) -> Option<FontFamilyFaces> {
        let id = *self.families.get(usize::try_from(family).ok()?)?;
        let info = self.fonts.collection.family(id)?;
        let mut faces = Vec::new();
        for font in info.fonts() {
            // A system's font is in a file; one of bytes alone isn't the
            // system's to give.
            let SourceKind::Path(path) = font.source().kind() else {
                continue;
            };
            let Some(file) = self.file_number(path) else {
                continue;
            };
            faces.push(FontFace {
                file,
                index: font.index(),
                weight: font.weight().value(),
                style: match font.style() {
                    FontStyle::Normal => FaceStyle::Normal,
                    FontStyle::Italic => FaceStyle::Italic,
                    FontStyle::Oblique(angle) => FaceStyle::Oblique(angle),
                },
                width: font.width().ratio(),
            });
        }
        (!faces.is_empty()).then(|| FontFamilyFaces {
            name: info.name().to_owned(),
            faces,
        })
    }

    fn file_number(&mut self, path: &Path) -> Option<u32> {
        if let Some(&number) = self.file_numbers.get(path) {
            return Some(number);
        }
        let number = u32::try_from(self.files.len()).ok()?;
        self.files.push(path.to_path_buf());
        self.file_numbers.insert(path.to_path_buf(), number);
        Some(number)
    }

    /// The families text of `script` falls back to (no language: machopo
    /// gives parley none).
    pub fn fallback(&mut self, script: [u8; 4]) -> Vec<u32> {
        let key = FallbackKey::new(Script::from_bytes(script), None);
        self.fonts
            .collection
            .fallback_families(key)
            .filter_map(|id| self.numbers.get(&id).copied())
            .collect()
    }

    /// How many files have been answered with: every file's number is
    /// below it.
    pub fn files_numbered(&self) -> u32 {
        u32::try_from(self.files.len()).unwrap_or(u32::MAX)
    }

    /// File `file`'s path, if it has been answered with.
    pub fn file(&self, file: u32) -> Option<&Path> {
        self.files
            .get(usize::try_from(file).ok()?)
            .map(PathBuf::as_path)
    }
}

/// A renderer's side (Chromium's `font_data_service::FontManager`): what
/// of the system's fonts its collection has been given, and how it asks
/// for more.
pub(crate) struct ServedFonts {
    service: Box<dyn FontDataService>,
    /// Each name, as fontique folds it to look it up, and its family.
    names: FxHashMap<String, u32>,
    generics: Vec<Vec<u32>>,
    /// The generic families set in the collection.
    generics_set: FxHashSet<parley::GenericFamily>,
    /// Each family asked for, and what it is in the collection (none: it
    /// has no face that could be loaded).
    loaded: FxHashMap<u32, Option<FamilyId>>,
    /// Each file asked for, and its bytes (none: it couldn't be opened).
    files: FxHashMap<u32, Option<Blob<u8>>>,
    /// The font collections registered: each family of its faces, and the
    /// faces' indices.
    collections: FxHashMap<u32, Vec<(FamilyId, Vec<u32>)>>,
    /// The scripts whose fallback has been set.
    scripts: FxHashSet<[u8; 4]>,
}

/// Latin, which text of no script of its own is shaped as.
const LATIN: [u8; 4] = *b"Latn";
/// Han, whose fallback parley adds to every script's.
const HAN: [u8; 4] = *b"Hani";

impl ServedFonts {
    pub(crate) fn new(names: FontNames, service: Box<dyn FontDataService>) -> Self {
        // The browser's numbers, taken as they are: a renderer trusts its
        // browser.
        let FontNames {
            names, generics, ..
        } = names;
        let names = names
            .into_iter()
            .map(|(name, family)| (fold(&name), family))
            .collect();
        Self {
            service,
            names,
            generics,
            generics_set: FxHashSet::default(),
            loaded: FxHashMap::default(),
            files: FxHashMap::default(),
            collections: FxHashMap::default(),
            scripts: FxHashSet::default(),
        }
    }

    /// Family `family` in `collection`, its faces registered there the
    /// first time.
    fn family(&mut self, collection: &mut Collection, family: u32) -> Option<FamilyId> {
        if let Some(&known) = self.loaded.get(&family) {
            return known;
        }
        let ours = self.load(collection, family);
        self.loaded.insert(family, ours);
        ours
    }

    fn load(&mut self, collection: &mut Collection, family: u32) -> Option<FamilyId> {
        let FontFamilyFaces { name, faces } = self.service.family(family)?;
        let mut missing: Vec<u32> = Vec::new();
        for face in &faces {
            if !self.files.contains_key(&face.file) && !missing.contains(&face.file) {
                missing.push(face.file);
            }
        }
        for files in missing.chunks(FONT_FILES_PER_CALL) {
            let mut given = self.service.files(files).into_iter();
            for &file in files {
                let bytes = given.next().flatten().map(Blob::new);
                self.files.insert(file, bytes);
            }
        }
        let mut ours = None;
        for face in &faces {
            let Some(Some(blob)) = self.files.get(&face.file) else {
                continue;
            };
            let id = if is_font_collection(blob.as_ref()) {
                // A collection's faces are the families they name
                // themselves, registered once for all of them: as the
                // system has them, by every comparison made.
                let families = self.collections.entry(face.file).or_insert_with(|| {
                    collection
                        .register_fonts(blob.clone(), None)
                        .into_iter()
                        .map(|(id, fonts)| (id, fonts.iter().map(|font| font.index()).collect()))
                        .collect()
                });
                families
                    .iter()
                    .find(|(_, indices)| indices.contains(&face.index))
                    .map(|&(id, _)| id)
            } else {
                // One face: the family's as the system has it, with the
                // system's name and what it says of the face (a family
                // has a file once; a file in several families is
                // registered for each).
                let info = FontInfoOverride {
                    family_name: Some(&name),
                    width: Some(FontWidth::from_ratio(face.width)),
                    style: Some(match face.style {
                        FaceStyle::Normal => FontStyle::Normal,
                        FaceStyle::Italic => FontStyle::Italic,
                        FaceStyle::Oblique(angle) => FontStyle::Oblique(angle),
                    }),
                    weight: Some(FontWeight::new(face.weight)),
                    axes: None,
                };
                collection
                    .register_fonts(blob.clone(), Some(info))
                    .first()
                    .map(|&(id, _)| id)
            };
            ours = ours.or(id);
        }
        ours
    }

    /// Before `text` is shaped: the fallback of every script parley will
    /// shape a part of it as, which it asks the collection for once
    /// shaping has begun (each segment's script, with Han's added). A
    /// character of no script of its own (`Zyyy`, `Zinh`, `Zzzz`) is
    /// shaped as the script of the characters around it; text of none at
    /// all as Latin.
    pub(crate) fn prepare_text(&mut self, collection: &mut Collection, text: &str) {
        let mut new: Vec<[u8; 4]> = Vec::new();
        if !self.scripts.contains(&HAN) {
            new.push(HAN);
        }
        let mut any_script = false;
        if text.is_ascii() {
            // ASCII is Latin and common characters alone.
            any_script = true;
            if !self.scripts.contains(&LATIN) {
                new.push(LATIN);
            }
        } else {
            let scripts = icu_properties::CodePointMapData::<icu_properties::props::Script>::new();
            let names = icu_properties::PropertyNamesShort::<icu_properties::props::Script>::new();
            for ch in text.chars() {
                if ch.is_ascii() {
                    if ch.is_ascii_alphabetic() {
                        any_script = true;
                        if !self.scripts.contains(&LATIN) && !new.contains(&LATIN) {
                            new.push(LATIN);
                        }
                    }
                    continue;
                }
                let script = scripts.get(ch);
                if matches!(
                    script,
                    icu_properties::props::Script::Common
                        | icu_properties::props::Script::Inherited
                        | icu_properties::props::Script::Unknown
                ) {
                    continue;
                }
                any_script = true;
                // As parley names it: the short name, else unknown (which
                // has no fallback).
                let Some(tag) = names
                    .get(script)
                    .and_then(|name| Script::parse(name).ok())
                    .map(Script::to_bytes)
                else {
                    continue;
                };
                if !self.scripts.contains(&tag) && !new.contains(&tag) {
                    new.push(tag);
                }
            }
        }
        if !any_script && !self.scripts.contains(&LATIN) && !new.contains(&LATIN) {
            new.push(LATIN);
        }
        for script in new {
            self.scripts.insert(script);
            let families = self.service.fallback(script);
            let ours: Vec<FamilyId> = families
                .into_iter()
                .filter_map(|family| self.family(collection, family))
                .collect();
            if !ours.is_empty() {
                collection.set_fallbacks(
                    FallbackKey::new(Script::from_bytes(script), None),
                    ours.into_iter(),
                );
            }
        }
    }

    /// How a family list's names and generic families are found: in what
    /// the browser gave, then among the fonts the collection has of its
    /// own (those registered here).
    pub(crate) fn lookup<'a>(&'a mut self, collection: &'a mut Collection) -> Served<'a> {
        Served {
            served: self,
            collection,
        }
    }
}

/// [`ServedFonts`] and the collection it fills, as a family list is looked
/// up.
pub(crate) struct Served<'a> {
    served: &'a mut ServedFonts,
    collection: &'a mut Collection,
}

impl FamilyLookup for Served<'_> {
    fn named(&mut self, name: &str) -> Option<FamilyId> {
        match self.served.names.get(&fold(name)) {
            Some(&family) => self.served.family(self.collection, family),
            None => self
                .collection
                .family_by_name(name)
                .map(|family| family.id()),
        }
    }

    fn generic(&mut self, generic: parley::GenericFamily) -> Vec<FamilyId> {
        if !self.served.generics_set.contains(&generic) {
            self.served.generics_set.insert(generic);
            let index = parley::GenericFamily::all()
                .iter()
                .position(|&each| each == generic);
            let families = index
                .and_then(|index| self.served.generics.get(index))
                .cloned()
                .unwrap_or_default();
            let ours: Vec<FamilyId> = families
                .into_iter()
                .filter_map(|family| self.served.family(self.collection, family))
                .collect();
            self.collection
                .set_generic_families(generic, ours.into_iter());
        }
        self.collection.generic_families(generic).collect()
    }
}

/// `name` as fontique folds a family name to look it up: each character
/// lower-cased.
fn fold(name: &str) -> String {
    name.chars().flat_map(char::to_lowercase).collect()
}

/// Whether `bytes` are a font collection (a `.ttc`): several faces, each
/// naming its own family.
fn is_font_collection(bytes: &[u8]) -> bool {
    matches!(
        read_fonts::FileRef::new(bytes),
        Ok(read_fonts::FileRef::Collection(_))
    )
}
