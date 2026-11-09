//! The XML parser: SVG files into the DOM, with xml5ever, the entities a
//! DOCTYPE declares expanded first (as libxml2, Chrome's, does).

use std::time::{Duration, Instant};

use dom::Dom;

/// The tree of `input` parsed as an SVG file, in html5lib's format (an SVG
/// element as `<svg name>`, an attribute's namespace before its name), and
/// the document's content type, as an SVG image's document is given it.
/// No HTML document: an element's names are as written.
fn parse(input: &str) -> (String, String) {
    let mut dom = Dom::new();
    let document = html::parser::parse_xml_document(&mut dom, input);
    let content_type = dom.mutate(|cx| {
        let document = document.root().get(cx.gc);
        assert!(!cx.is_html_document(document));
        cx.set_document_content_type(document, "image/svg+xml");
        cx.document_content_type(document).to_string()
    });
    (crate::harness::dump(&mut dom, &document), content_type)
}

#[test]
fn an_svg_file_becomes_a_tree() {
    let (dump, content_type) = parse(
        "\u{FEFF}<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!-- before -->\n\
         <?xml-stylesheet href=\"a.css\"?>\n\
         <svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"10\">\
         <use xlink:href=\"#a\"/><text>a&amp;b&#65;<![CDATA[<c>]]></text></svg>",
    );
    assert_eq!(content_type, "image/svg+xml");
    // No byte order mark, no node for the XML declaration; the namespaces
    // as declared, the declarations themselves not kept as attributes
    // (libxml2 keeps them; a document an image is made of is script's to
    // see no more than xml5ever's); a CDATA section as text (xml5ever makes
    // no nodes of them).
    assert_eq!(
        dump,
        "\
| <!--  before  -->
| <?xml-stylesheet href=\"a.css\">
| <svg svg>
|   width=\"10\"
|   <svg use>
|     xlink href=\"#a\"
|   <svg text>
|     \"a&bA<c>\"
"
    );
}

/// A DOCTYPE's general entities, as Illustrator writes them (the SVG
/// namespace as one), expanded in content and attribute values, in each
/// other, not in comments, CDATA sections or processing instructions; the
/// internal subset gone from the DOCTYPE. External and parameter entities,
/// and undeclared ones, are left as they are; the first declaration wins.
#[test]
fn a_doctypes_entities_are_expanded() {
    let (dump, _) = parse(
        "<?xml version=\"1.0\"?>\n\
         <!-- a comment --><?pi before?>\n\
         <!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \"svg[11].dtd\" [\n\
         \t<!ATTLIST svg a CDATA \"]>\">\n\
         \t<!ENTITY ns_svg \"http://www.w3.org/2000/svg\">\n\
         \t<!ENTITY ns_svg \"not this one\">\n\
         \t<!-- ] > in a comment -->\n\
         \t<!ENTITY w '1&n;'>\n\
         \t<!ENTITY n \"0\">\n\
         \t<!ENTITY % p \"parameter\">\n\
         \t<!ENTITY ext SYSTEM \"ext.xml\">\n\
         ]>\n\
         <svg xmlns=\"&ns_svg;\" width=\"&w;\"><!-- &w; --><text>&w;&ext;&p;<![CDATA[&w;]]></text><?pi &w;?></svg>",
    );
    assert_eq!(
        dump,
        "\
| <!--  a comment  -->
| <?pi before>
| <!DOCTYPE svg \"-//W3C//DTD SVG 1.1//EN\" \"svg[11].dtd\">
| <svg svg>
|   width=\"10\"
|   <!--  &w;  -->
|   <svg text>
|     \"10&ext;&p;&w;\"
|   <?pi &w;>
"
    );
}

/// An entity made of ten of another, ten levels deep ("billion laughs"),
/// stops expanding when its expansion grows too big, rather than taking
/// gigabytes: the rest of the references are left as they are.
#[test]
fn entities_stop_expanding_when_too_big() {
    let mut subset = String::from("<!ENTITY l0 \"lol\">");
    for level in 1..10 {
        subset.push_str(&format!(
            "<!ENTITY l{level} \"{}\">",
            format!("&l{};", level - 1).repeat(10)
        ));
    }
    let input = format!(
        "<!DOCTYPE svg [{subset}]><svg xmlns=\"http://www.w3.org/2000/svg\"><text>&l9;</text></svg>"
    );
    let start = Instant::now();
    let (dump, _) = parse(&input);
    assert!(start.elapsed() < Duration::from_secs(5));
    assert!(dump.len() < 4 << 20, "{} bytes", dump.len());
    assert!(dump.contains("lollol"), "some of it expanded");
    assert!(dump.contains("&l"), "the rest left as references");
}

/// Malformed XML is parsed as xml5ever recovers it (Chrome breaks the image
/// at the first error): a missing end tag, an end tag of another element.
#[test]
fn malformed_xml_is_recovered() {
    let (dump, _) = parse("<svg xmlns=\"http://www.w3.org/2000/svg\"><g><rect/></svg><circle/>");
    assert!(
        dump.starts_with("| <svg svg>\n|   <svg g>\n|     <svg rect>\n"),
        "{dump}"
    );
    // An element out of the SVG namespace is in none.
    let mut dom = Dom::new();
    let document = html::parser::parse_xml_document(&mut dom, "<svg/>");
    dom.read(|cx| {
        let root = cx.first_child(document.root().get(cx.gc)).unwrap();
        let namespace = cx.element_name(root).unwrap().namespace;
        assert_eq!(&*dom::names::get(namespace), "");
    });
}

/// An entity that refers to itself, or to one that refers back, stops
/// expanding some levels down rather than for ever.
#[test]
fn entities_referring_to_themselves_stop() {
    let (dump, _) = parse(
        "<!DOCTYPE svg [<!ENTITY a \"x&b;\"><!ENTITY b \"&a;\">]>\
         <svg xmlns=\"http://www.w3.org/2000/svg\"><text>&a;</text></svg>",
    );
    assert!(dump.contains("xxxx"), "{dump}");
    assert!(dump.contains("&"), "the last reference left as it is");
    assert!(dump.len() < 1000, "{} bytes", dump.len());
}
