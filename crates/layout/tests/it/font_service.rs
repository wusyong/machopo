//! The system's fonts as the browser serves them (`font_service`): a text
//! context that asks for them lays text out as one that has them itself,
//! and asks for each family, script and file once, as text needs it.

use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use computed_style::{CanvasFont, CanvasTextStyle, Direction, FontFamily, FontKerning};
use gfx::FontData;
use layout::{
    BoxFragment, FaceStyle, FontDataService, FontFace, FontFamilyFaces, FontFileBytes, FontNames,
    SvgNode, SvgNodeKind, SystemFontManager, TextContext, shape_canvas_text,
};

use crate::page::{Page, VIEWPORT};

/// What the browser was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Asked {
    Family(u32),
    Fallback(String),
    Files(Vec<u32>),
}

type Log = Rc<RefCell<Vec<Asked>>>;

/// The browser's fonts, answered at once, and what it was asked.
struct Direct {
    fonts: SystemFontManager,
    log: Log,
}

impl FontDataService for Direct {
    fn family(&mut self, family: u32) -> Option<FontFamilyFaces> {
        self.log.borrow_mut().push(Asked::Family(family));
        self.fonts.family(family)
    }

    fn fallback(&mut self, script: [u8; 4]) -> Vec<u32> {
        self.log.borrow_mut().push(Asked::Fallback(
            String::from_utf8_lossy(&script).into_owned(),
        ));
        self.fonts.fallback(script)
    }

    fn files(&mut self, files: &[u32]) -> Vec<Option<FontFileBytes>> {
        self.log.borrow_mut().push(Asked::Files(files.to_vec()));
        files
            .iter()
            .map(|&file| {
                let bytes = std::fs::read(self.fonts.file(file)?).ok()?;
                let bytes: FontFileBytes = Arc::new(bytes);
                Some(bytes)
            })
            .collect()
    }
}

fn test_fonts() -> &'static Path {
    Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../resources/test_fonts"
    ))
}

/// Every generic family's CSS keyword.
const GENERICS: &[&str] = &[
    "serif",
    "sans-serif",
    "monospace",
    "cursive",
    "fantasy",
    "system-ui",
    "ui-serif",
    "ui-sans-serif",
    "ui-monospace",
    "ui-rounded",
    "emoji",
    "math",
    "fangsong",
];

/// The test fonts as the browser's, every generic family Lato (as
/// `TextContext::new(true)` has them), what it is asked, and the names it
/// gave.
fn served_test_fonts() -> (TextContext, Log, FontNames) {
    let generics: Vec<(&str, &str)> = GENERICS.iter().map(|&generic| (generic, "Lato")).collect();
    served(SystemFontManager::for_testing(&[test_fonts()], &generics))
}

fn served(fonts: SystemFontManager) -> (TextContext, Log, FontNames) {
    let log = Log::default();
    let names = fonts.names().clone();
    let service = Direct {
        fonts,
        log: log.clone(),
    };
    (
        TextContext::with_font_service(names.clone(), Box::new(service)),
        log,
        names,
    )
}

fn family_of(names: &FontNames, name: &str) -> u32 {
    names
        .names
        .iter()
        .find(|(each, _)| each == name)
        .unwrap_or_else(|| panic!("no {name}"))
        .1
}

/// A font, by what its bytes are.
#[derive(Debug, PartialEq, Eq)]
struct Font {
    len: usize,
    hash: u64,
    index: u32,
}

impl Font {
    fn of(font: &FontData) -> Self {
        let bytes: &[u8] = (*font.bytes).as_ref();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        bytes[..bytes.len().min(4096)].hash(&mut hasher);
        Self {
            len: bytes.len(),
            hash: hasher.finish(),
            index: font.index,
        }
    }
}

/// A run of glyphs of one font, as it is drawn.
#[derive(Debug, PartialEq)]
struct Run {
    font: Font,
    glyphs: Vec<u32>,
    xs: Vec<f32>,
    embolden: bool,
    skew: f32,
}

/// Every run of `view`'s lines and of its SVG text, in tree order.
fn runs(view: &BoxFragment) -> Vec<Run> {
    fn svg(nodes: &[SvgNode], out: &mut Vec<Run>) {
        for node in nodes {
            match &node.kind {
                SvgNodeKind::Container(children) | SvgNodeKind::Hidden(children) => {
                    svg(children, out);
                }
                SvgNodeKind::Text(text) => out.extend(text.runs.iter().map(|run| Run {
                    font: Font::of(&run.font),
                    glyphs: run.glyphs.iter().map(|&glyph| u32::from(glyph)).collect(),
                    xs: run.positions.iter().map(|at| at.x).collect(),
                    embolden: run.embolden,
                    skew: run.skew,
                })),
                SvgNodeKind::Shape(_) => {}
            }
        }
    }
    fn walk(fragment: &BoxFragment, out: &mut Vec<Run>) {
        for run in fragment.lines.iter().flat_map(|line| &line.runs) {
            let glyphs: Vec<_> = run.glyphs().collect();
            out.push(Run {
                font: Font::of(&run.font),
                glyphs: glyphs.iter().map(|&(glyph, _)| glyph).collect(),
                xs: glyphs.iter().map(|(_, at)| at.x).collect(),
                embolden: run.embolden,
                skew: run.skew,
            });
        }
        if let Some(content) = &fragment.svg {
            svg(&content.children, out);
        }
        for child in &fragment.children {
            walk(&child.fragment, out);
        }
    }
    let mut out = Vec::new();
    walk(view, &mut out);
    out
}

fn laid_out(html: &str, text: TextContext) -> (Vec<Run>, TextContext) {
    let mut page = Page::build_with_text(html, text);
    let view = page.tree.layout(VIEWPORT);
    let runs = runs(&view);
    (runs, page.text)
}

fn canvas_style(family: FontFamily) -> CanvasTextStyle {
    CanvasTextStyle {
        font: CanvasFont {
            family: Arc::from([family]),
            size: 20.0,
            ..CanvasFont::default()
        },
        direction: Direction::Ltr,
        letter_spacing: 0.0,
        word_spacing: 0.0,
        kerning: FontKerning::Auto,
    }
}

fn canvas_runs(cx: &mut TextContext, text: &str, family: FontFamily) -> Vec<Run> {
    shape_canvas_text(cx, text, &canvas_style(family))
        .runs
        .iter()
        .map(|run| Run {
            font: Font::of(&run.font),
            glyphs: run.glyphs.iter().map(|&glyph| u32::from(glyph)).collect(),
            xs: run.positions.iter().map(|at| at.x).collect(),
            embolden: run.embolden,
            skew: run.skew,
        })
        .collect()
}

const PAGE: &str = "<div style='font: 20px Ahem'>Ahem <b>bold</b> <i>it</i></div>\
    <div style='font: 20px Lato'>Lato</div><div style='font: 20px serif'>serif</div>\
    <div style='font: 20px nothing-of-the-name'>none</div>\
    <svg><text y=20 style='font: 20px Ahem'>svg</text></svg>";

/// The test fonts served lay text out as the test fonts the context has
/// itself, on every platform: the faces, as the browser has them, are
/// registered as they would be found.
#[test]
fn served_fonts_lay_text_out_as_found_ones() {
    let (served, ..) = served_test_fonts();
    let (own, _) = laid_out(PAGE, TextContext::new(true));
    let (theirs, mut served) = laid_out(PAGE, served);
    assert!(!own.is_empty());
    assert_eq!(theirs, own);
    let mut test_fonts = TextContext::new(true);
    for family in [
        FontFamily::Named("Ahem".into()),
        FontFamily::Named("Lato".into()),
    ] {
        assert_eq!(
            canvas_runs(&mut served, "canvas", family.clone()),
            canvas_runs(&mut test_fonts, "canvas", family)
        );
    }
}

/// A family is asked for the first time a value names it (in any case),
/// with its files; a generic family the first time one is used, by the
/// families it stands for; a script's fallback, and Han's, before the
/// first text of it is shaped. Nothing is asked twice.
#[test]
fn each_family_script_and_file_is_asked_for_once() {
    let (text, log, names) = served_test_fonts();
    let (ahem, lato) = (family_of(&names, "Ahem"), family_of(&names, "Lato"));
    let (_, text) = laid_out("<div style='font: 20px AHEM'>a</div>", text);
    // Each family's one file is the first file numbered for it.
    let asked = log.borrow().clone();
    assert_eq!(
        asked,
        [
            Asked::Fallback("Hani".into()),
            Asked::Fallback("Latn".into()),
            Asked::Family(ahem),
            Asked::Files(vec![0]),
            // `serif`, which every list ends in: Lato.
            Asked::Family(lato),
            Asked::Files(vec![1]),
        ]
    );
    log.borrow_mut().clear();
    let (_, mut text) = laid_out(
        "<div style='font: 20px ahem'>b</div><div style='font: 20px Lato'>c</div>",
        text,
    );
    assert_eq!(*log.borrow(), []);
    canvas_runs(&mut text, "αβγ", FontFamily::Named("Ahem".into()));
    assert_eq!(*log.borrow(), [Asked::Fallback("Grek".into())]);
}

/// The scripts whose fallback is asked for before a text is shaped: each
/// script it has a character of; Latin for one of none (as parley shapes
/// it); and Han's with the first. SVG and a canvas shape a text of their
/// own after it (an `x` for the line height, a space for the metrics),
/// which may ask for Latin's.
#[test]
fn a_texts_scripts_are_asked_for_before_it_is_shaped() {
    let fallbacks = |log: &Log| -> Vec<String> {
        log.borrow()
            .iter()
            .filter_map(|asked| match asked {
                Asked::Fallback(script) => Some(script.clone()),
                Asked::Family(_) | Asked::Files(_) => None,
            })
            .collect()
    };
    for (text, scripts) in [
        ("abc", &["Hani", "Latn"][..]),
        ("123 !", &["Hani", "Latn"]),
        ("— …", &["Hani", "Latn"]),
        ("αβγ 123", &["Hani", "Grek"]),
        ("é", &["Hani", "Latn"]),
        ("a αβγ", &["Hani", "Latn", "Grek"]),
        ("שלום и мир", &["Hani", "Hebr", "Cyrl"]),
        ("中文", &["Hani"]),
        ("かな", &["Hani", "Hira"]),
    ] {
        // In a block's lines.
        let (cx, log, _) = served_test_fonts();
        laid_out(&format!("<div style='font: 20px Ahem'>{text}</div>"), cx);
        assert_eq!(fallbacks(&log), scripts, "{text} in a block");
        // In SVG.
        let (cx, log, _) = served_test_fonts();
        laid_out(
            &format!("<svg><text y=20 style='font: 20px Ahem'>{text}</text></svg>"),
            cx,
        );
        assert_eq!(fallbacks(&log)[..scripts.len()], *scripts, "{text} in SVG");
        // On a canvas.
        let (mut cx, log, _) = served_test_fonts();
        canvas_runs(&mut cx, text, FontFamily::Named("Ahem".into()));
        assert_eq!(
            fallbacks(&log)[..scripts.len()],
            *scripts,
            "{text} on a canvas"
        );
    }
}

/// A browser of names and families made up for the test: every face's
/// file is Ahem.
struct MadeUp {
    families: Vec<FontFamilyFaces>,
    log: Log,
    /// The files it can't open.
    unopened: Vec<u32>,
}

impl FontDataService for MadeUp {
    fn family(&mut self, family: u32) -> Option<FontFamilyFaces> {
        self.log.borrow_mut().push(Asked::Family(family));
        self.families.get(usize::try_from(family).ok()?).cloned()
    }

    fn fallback(&mut self, _: [u8; 4]) -> Vec<u32> {
        Vec::new()
    }

    fn files(&mut self, files: &[u32]) -> Vec<Option<FontFileBytes>> {
        self.log.borrow_mut().push(Asked::Files(files.to_vec()));
        let ahem = std::fs::read(test_fonts().join("Ahem.ttf")).unwrap();
        files
            .iter()
            .map(|file| {
                let bytes: FontFileBytes = Arc::new(ahem.clone());
                (!self.unopened.contains(file)).then_some(bytes)
            })
            .collect()
    }
}

fn faces(name: &str, files: std::ops::Range<u32>) -> FontFamilyFaces {
    FontFamilyFaces {
        name: name.to_owned(),
        faces: files
            .map(|file| FontFace {
                file,
                index: 0,
                weight: 400.0,
                style: FaceStyle::Normal,
                width: 1.0,
            })
            .collect(),
    }
}

fn made_up(
    names: &[(&str, u32)],
    families: Vec<FontFamilyFaces>,
    generics: Vec<Vec<u32>>,
    unopened: Vec<u32>,
) -> (TextContext, Log) {
    let log = Log::default();
    let names = FontNames {
        names: names
            .iter()
            .map(|&(name, family)| (name.to_owned(), family))
            .collect(),
        families: u32::try_from(families.len()).unwrap(),
        generics,
    };
    let service = MadeUp {
        families,
        log: log.clone(),
        unopened,
    };
    (
        TextContext::with_font_service(names, Box::new(service)),
        log,
    )
}

fn files_asked(log: &Log) -> Vec<Vec<u32>> {
    log.borrow()
        .iter()
        .filter_map(|asked| match asked {
            Asked::Files(files) => Some(files.clone()),
            Asked::Family(_) | Asked::Fallback(_) => None,
        })
        .collect()
}

/// A family's files are asked for as many at once as go with a message;
/// one that couldn't be opened leaves its faces out, and a family with
/// none left isn't there.
#[test]
fn files_are_asked_for_sixteen_at_a_time() {
    let many = faces("Many", 0..20);
    let many = || {
        made_up(
            &[("Many", 0), ("Gone", 1)],
            vec![many.clone(), faces("Gone", 20..22)],
            Vec::new(),
            vec![20, 21],
        )
    };
    let (cx, log) = many();
    let (runs, _) = laid_out("<div style='font: 20px Many'>x</div>", cx);
    assert_eq!(
        files_asked(&log),
        [(0..16).collect::<Vec<_>>(), (16..20).collect()]
    );
    // Ahem, under the family's name.
    assert_eq!(runs[0].font, font_file("Ahem.ttf"));
    let (cx, log) = many();
    let (_, cx) = laid_out("<div style='font: 20px Gone'>x</div>", cx);
    assert_eq!(
        *log.borrow(),
        [Asked::Family(1), Asked::Files(vec![20, 21])]
    );
    // A family with no face isn't asked for again.
    log.borrow_mut().clear();
    laid_out("<div style='font: 20px gone'>y</div>", cx);
    assert_eq!(*log.borrow(), []);
}

/// A browser of one family, `Mixed`, whose faces are of files that name
/// other families: Ahem, and Lato, a file of two of its faces.
struct Mixed {
    log: Log,
}

impl FontDataService for Mixed {
    fn family(&mut self, family: u32) -> Option<FontFamilyFaces> {
        self.log.borrow_mut().push(Asked::Family(family));
        let face = |file, weight, style| FontFace {
            file,
            index: 0,
            weight,
            style,
            width: 1.0,
        };
        Some(FontFamilyFaces {
            name: "Mixed".to_owned(),
            faces: vec![
                face(0, 400.0, FaceStyle::Normal),
                face(1, 700.0, FaceStyle::Normal),
                face(1, 700.0, FaceStyle::Italic),
            ],
        })
    }

    fn fallback(&mut self, _: [u8; 4]) -> Vec<u32> {
        Vec::new()
    }

    fn files(&mut self, files: &[u32]) -> Vec<Option<FontFileBytes>> {
        self.log.borrow_mut().push(Asked::Files(files.to_vec()));
        files
            .iter()
            .map(|&file| {
                let name = if file == 0 {
                    "Ahem.ttf"
                } else {
                    "Lato-Medium.ttf"
                };
                let bytes: FontFileBytes = Arc::new(std::fs::read(test_fonts().join(name)).ok()?);
                Some(bytes)
            })
            .collect()
    }
}

/// A family's faces are the family's, as the browser has them, whatever
/// family their files name (a family of DirectWrite's may group files of
/// several typographic families); a file of several of its faces is asked
/// for once.
#[test]
fn a_familys_faces_are_its_own_whatever_their_files_name() {
    let log = Log::default();
    let names = FontNames {
        names: vec![("Mixed".to_owned(), 0)],
        families: 1,
        generics: Vec::new(),
    };
    let cx = TextContext::with_font_service(names, Box::new(Mixed { log: log.clone() }));
    let (runs, _) = laid_out("<div style='font: 20px Mixed'>x <b>x</b></div>", cx);
    assert_eq!(*log.borrow(), [Asked::Family(0), Asked::Files(vec![0, 1])]);
    let fonts: Vec<&Font> = runs.iter().map(|run| &run.font).collect();
    assert_eq!(
        fonts,
        [&font_file("Ahem.ttf"), &font_file("Lato-Medium.ttf")]
    );
}

fn font_file(name: &str) -> Font {
    Font::of(&FontData {
        bytes: Arc::new(std::fs::read(test_fonts().join(name)).unwrap()),
        index: 0,
        id: 0,
    })
}

/// A name is any of a family's, in any case, as the browser's list has it;
/// one not in the list is looked up among the fonts the collection has of
/// its own.
#[test]
fn names_are_the_browsers_then_the_collections_own() {
    let (cx, log) = made_up(
        &[("First", 0), ("Other Name", 0)],
        vec![faces("First", 0..1)],
        Vec::new(),
        Vec::new(),
    );
    let (runs, mut cx) = laid_out(
        "<div style='font: 20px OTHER NAME'>x</div><div style='font: 20px first'>x</div>",
        cx,
    );
    assert_eq!(*log.borrow(), [Asked::Family(0), Asked::Files(vec![0])]);
    assert_eq!(runs[0].font, runs[1].font);
    let lato = std::fs::read(test_fonts().join("Lato-Medium.ttf")).unwrap();
    assert_eq!(cx.register_fonts_for_testing(lato), ["Lato"]);
    log.borrow_mut().clear();
    let (runs, _) = laid_out("<div style='font: 20px Lato'>x</div>", cx);
    assert_eq!(*log.borrow(), []);
    assert_eq!(runs[0].font, font_file("Lato-Medium.ttf"));
}

/// A generic family's families are asked for the first time it is used;
/// one never used, never (every list ends in `serif`).
#[test]
fn a_generic_family_is_asked_for_when_it_is_used() {
    let mut generics = vec![Vec::new(); GENERICS.len()];
    // In parley's order: `sans-serif`, `serif`, `monospace`, `cursive`.
    generics[1] = vec![0];
    generics[2] = vec![1];
    generics[3] = vec![2];
    let (cx, log) = made_up(
        &[],
        vec![faces("S", 0..1), faces("M", 1..2), faces("C", 2..3)],
        generics,
        Vec::new(),
    );
    laid_out("<div style='font: 20px monospace'>x</div>", cx);
    let families: Vec<u32> = log
        .borrow()
        .iter()
        .filter_map(|asked| match asked {
            Asked::Family(family) => Some(*family),
            Asked::Fallback(_) | Asked::Files(_) => None,
        })
        .collect();
    assert_eq!(families, [1, 0]);
}

/// The system's fonts as the browser serves them lay text out as the
/// system's fonts found in the renderer do: a page of many scripts, the
/// generic families, Chrome's alternate names, faces of a font collection
/// and names that aren't there, in a block, in SVG and on a canvas. Where
/// renderers are served the system's fonts.
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
#[test]
fn served_system_fonts_lay_text_out_as_the_systems_own() {
    let page = "<style>div { font: 24px serif }</style>\
        <div>Latin text, digits 0123 and punctuation: “quotes” — ‘dash’…</div>\
        <div style='font-family: sans-serif'>sans-serif <b>bold</b> <i>italic</i></div>\
        <div style='font-family: monospace'>monospace mmm iii</div>\
        <div style='font-family: cursive'>cursive</div><div style='font-family: fantasy'>fantasy</div>\
        <div style='font-family: system-ui'>system-ui</div>\
        <div style='font-family: Arial'>Arial</div><div style='font-family: Helvetica'>Helvetica</div>\
        <div style='font-family: Times'>Times</div><div style='font-family: Courier'>Courier</div>\
        <div style='font-family: \"Segoe UI\"'>Segoe UI <b>bold</b></div>\
        <div style='font-family: \"Cambria Math\"'>∑∫√ Cambria Math</div>\
        <div style='font-family: nothing-of-the-name, sans-serif'>missing</div>\
        <div>中文字 漢字 日本語のかなカナ 한국어 العربية עברית ไทย हिन्दी ελληνικά русский</div>\
        <div>ქართული Հայերեն বাংলা தமிழ் ລາວ ខ្មែរ ᠮᠣᠩᠭᠣᠯ ☃ ★ → ⅓ ①</div>\
        <div style='font-family: \"MS Gothic\"'>ＭＳ ゴシック</div>\
        <div style='font-family: \"微軟正黑體\"'>微軟正黑體</div>\
        <div style='font-family: \"Noto Sans CJK JP\"'>日本語 <b>太字</b></div>\
        <div style='font-family: \"Noto Serif CJK TC\"; font-weight: 300'>細明體</div>\
        <div style='font-family: \"DejaVu Sans\"'>DejaVu</div>\
        <div style='font-family: \"Liberation Mono\"'>Liberation</div>\
        <svg><text y=24 style='font: 24px serif'>SVG 中文 العربية</text></svg>";
    let (served, ..) = served(SystemFontManager::new(layout::SystemFonts::find()));
    let found = TextContext::with_system_fonts(layout::SystemFonts::find());
    let (theirs, mut served) = laid_out(page, served);
    let (own, mut found) = laid_out(page, found);
    assert!(own.len() > 20, "{} runs", own.len());
    for (index, (theirs, own)) in theirs.iter().zip(&own).enumerate() {
        assert_eq!(theirs, own, "run {index}");
    }
    assert_eq!(theirs.len(), own.len());
    for (text, family) in [
        (
            "canvas",
            FontFamily::Generic(computed_style::GenericFamily::SansSerif),
        ),
        (
            "中文 العربية ไทย",
            FontFamily::Generic(computed_style::GenericFamily::Serif),
        ),
        ("Courier", FontFamily::Named("Courier".into())),
    ] {
        assert_eq!(
            canvas_runs(&mut served, text, family.clone()),
            canvas_runs(&mut found, text, family),
            "{text} on a canvas"
        );
    }
}
