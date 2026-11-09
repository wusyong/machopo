//! HTML's element interfaces by their local names and their members:
//! reflections (strings, booleans, `long`s, clamped `unsigned long`s,
//! URLs, null as the empty string), `HTMLElement`'s global attributes,
//! tables' sections, rows and cells, a map's areas, a `<body>`'s window
//! event handlers, and the document's `images`, `embeds`, `plugins` and
//! `scripts`.

use crate::history::Page;

#[test]
fn interfaces_by_local_name_and_reflections() {
    let mut page = Page::new("https://example.test/a/");
    for (tag, interface) in [
        ("div", "HTMLDivElement"),
        ("h3", "HTMLHeadingElement"),
        ("td", "HTMLTableCellElement"),
        ("th", "HTMLTableCellElement"),
        ("xmp", "HTMLPreElement"),
        ("del", "HTMLModElement"),
        ("tbody", "HTMLTableSectionElement"),
        ("colgroup", "HTMLTableColElement"),
        ("section", "HTMLElement"),
    ] {
        assert_eq!(
            page.eval(&format!(
                "Object.prototype.toString.call(document.createElement('{tag}'))"
            )),
            format!("[object {interface}]"),
            "{tag}"
        );
    }
    assert_eq!(
        page.eval(
            "var td = document.createElement('td'); var c = [td.colSpan, td.rowSpan]; \
             td.setAttribute('colspan', '0'); td.setAttribute('rowspan', '99999999999999999999'); \
             c.push(td.colSpan, td.rowSpan); td.colSpan = 3; c.push(td.getAttribute('colspan')); \
             td.setAttribute('colspan', '-2'); c.push(td.colSpan); c.join()"
        ),
        "1,1,1,65534,3,1"
    );
    assert_eq!(
        page.eval(
            "var ol = document.createElement('ol'); var o = [ol.start, ol.reversed]; ol.start = -4; ol.reversed = true; \
             o.push(ol.getAttribute('start'), ol.hasAttribute('reversed')); ol.setAttribute('start', 'x'); o.push(ol.start); \
             var q = document.createElement('q'); q.cite = 'b'; o.push(q.cite); \
             var body = document.createElement('body'); body.bgColor = null; o.push(JSON.stringify(body.getAttribute('bgcolor'))); \
             var cell = document.createElement('th'); cell.scope = 'ROW'; o.push(cell.scope); cell.scope = 'x'; o.push(JSON.stringify(cell.scope)); \
             var t = document.createElement('title'); t.text = 'x<'; o.push(t.innerHTML); o.join()"
        ),
        "1,false,-4,true,1,https://example.test/a/b,\"\",row,\"\",x&lt;"
    );
}

#[test]
fn html_elements_global_attributes() {
    let mut page = Page::new("https://example.test/");
    assert_eq!(
        page.eval(
            "var outer = document.body.appendChild(document.createElement('div')); var inner = outer.appendChild(document.createElement('span')); \
             var r = [inner.translate, inner.spellcheck]; outer.setAttribute('translate', 'NO'); outer.setAttribute('spellcheck', 'false'); \
             r.push(inner.translate, inner.spellcheck); inner.translate = true; inner.spellcheck = true; \
             r.push(inner.getAttribute('translate'), inner.getAttribute('spellcheck'), inner.translate, inner.spellcheck); r.join()"
        ),
        "true,true,false,false,yes,true,true,true"
    );
    assert_eq!(
        page.eval(
            "var d = document.createElement('div'); var h = [d.hidden]; d.hidden = 'UNTIL-found'; h.push(d.hidden, d.getAttribute('hidden')); \
             d.hidden = 0; h.push(d.hasAttribute('hidden')); d.hidden = 'x'; h.push(d.hidden, d.getAttribute('hidden')); \
             d.hidden = NaN; h.push(d.hasAttribute('hidden')); d.hidden = true; h.push(d.hidden); d.hidden = null; h.push(d.hidden); \
             h.join()"
        ),
        "false,until-found,until-found,false,true,,false,true,false"
    );
    assert_eq!(
        page.eval(
            "var a = document.createElement('a'); var img = document.createElement('img'); var g = [a.draggable, img.draggable]; \
             a.href = 'x'; g.push(a.draggable); img.draggable = false; g.push(img.getAttribute('draggable'), img.draggable); \
             d.dir = 'RTL'; g.push(d.dir); d.dir = 'up'; g.push(JSON.stringify(d.dir)); \
             d.inputMode = 'Numeric'; d.enterKeyHint = 'GO'; g.push(d.inputMode, d.enterKeyHint, d.getAttribute('enterkeyhint')); \
             d.title = 't'; d.lang = 'fr'; d.accessKey = 'k'; d.inert = true; d.autofocus = true; \
             g.push(d.title, d.lang, d.accessKey, d.hasAttribute('inert'), d.hasAttribute('autofocus')); g.join()"
        ),
        "false,true,true,false,false,rtl,\"\",numeric,go,GO,t,fr,k,true,true"
    );
    // `autocapitalize`: a form control's is its form owner's without one.
    assert_eq!(
        page.eval(
            "var f = document.body.appendChild(document.createElement('form')); var i = f.appendChild(document.createElement('input')); \
             var c = [JSON.stringify(i.autocapitalize)]; f.autocapitalize = 'off'; c.push(f.autocapitalize, i.autocapitalize); \
             i.autocapitalize = 'WORDS'; c.push(i.autocapitalize); i.autocapitalize = 'x'; c.push(i.autocapitalize); \
             var p = f.appendChild(document.createElement('p')); c.push(JSON.stringify(p.autocapitalize)); c.join()"
        ),
        "\"\",none,none,words,sentences,\"\""
    );
}

#[test]
fn tables() {
    let mut page = Page::new("https://example.test/");
    page.eval(
        "var t = document.body.appendChild(document.createElement('table')); \
         var add = (parent, name, id) => { var e = parent.appendChild(document.createElement(name)); if (id) e.id = id; return e; }; \
         add(add(t, 'tfoot'), 'tr', 'f'); add(t, 'tr', 'own'); var body = add(t, 'tbody'); add(body, 'tr', 'b1'); add(body, 'tr', 'b2'); \
         add(add(t, 'thead'), 'tr', 'h'); \
         var ids = (c) => [...c].map((x) => x.id).join(' '); 0",
    );
    assert_eq!(page.eval("ids(t.rows)"), "h own b1 b2 f");
    assert_eq!(
        page.eval("[t.rows === t.rows, t.tBodies.length, t.tBodies === t.tBodies, t.tHead.id || t.tHead.localName, t.tFoot.localName].join()"),
        "true,1,true,thead,tfoot"
    );
    // `insertRow`: -1 after the last row (the tfoot's), an index before
    // that row, the count after the last; past it an `IndexSizeError`.
    assert_eq!(
        page.eval(
            "var last = t.insertRow(); last.id = 'L'; var second = t.insertRow(1); second.id = 'S'; \
             var end = t.insertRow(t.rows.length); end.id = 'E'; \
             [ids(t.rows), last.parentNode.localName, second.parentNode === t, end.parentNode.localName].join('|')"
        ),
        "h S own b1 b2 f L E|tfoot|true|tfoot"
    );
    assert_eq!(page.eval("t.insertRow(99)"), "!IndexSizeError");
    assert_eq!(page.eval("t.insertRow(-2)"), "!IndexSizeError");
    assert_eq!(
        page.eval("t.deleteRow(0); t.deleteRow(-1); ids(t.rows)"),
        "S own b1 b2 f L"
    );
    assert_eq!(page.eval("t.deleteRow(40)"), "!IndexSizeError");
    // Row and cell indexes.
    assert_eq!(
        page.eval(
            "var b2 = document.getElementById('b2'); var td = b2.insertCell(); var th = b2.appendChild(document.createElement('th')); \
             var first = b2.insertCell(0); b2.prepend(document.createElement('span')); [b2.rowIndex, b2.sectionRowIndex, b2.cells.length, first.cellIndex, td.cellIndex, th.cellIndex, \
             document.createElement('td').cellIndex, document.createElement('tr').rowIndex].join()"
        ),
        "3,1,3,0,1,2,-1,-1"
    );
    assert_eq!(
        page.eval("b2.deleteCell(-1); b2.deleteCell(0); b2.cells.length"),
        "1"
    );
    // A section's rows; a table without rows gets a `tbody`.
    assert_eq!(
        page.eval(
            "var e = document.createElement('table'); var r = e.insertRow(); \
             [r.parentNode.localName, e.tBodies.length, e.tBodies[0].rows.length, e.tBodies[0].insertRow(0).sectionRowIndex].join()"
        ),
        "tbody,1,1,0"
    );
    // Caption and head: where they go; a wrong element an error.
    assert_eq!(
        page.eval(
            "var n = document.createElement('table'); n.appendChild(document.createElement('colgroup')); n.appendChild(document.createElement('tbody')); \
             var h = n.createTHead(); var c = n.createCaption(); var f = n.createTFoot(); var b = n.createTBody(); \
             [[...n.children].map((x) => x.localName).join(' '), n.createTHead() === h, n.caption === c].join('|')"
        ),
        "caption colgroup thead tbody tbody tfoot|true|true"
    );
    assert_eq!(
        page.eval("n.tHead = document.createElement('tfoot')"),
        "!HierarchyRequestError"
    );
    assert_eq!(
        page.eval("n.deleteCaption(); n.deleteTHead(); n.tFoot = null; n.caption = document.createElement('caption'); [...n.children].map((x) => x.localName).join(' ')"),
        "caption colgroup tbody tbody"
    );
}

#[test]
fn collections_body_handlers_and_maps() {
    let mut page = Page::new("https://example.test/");
    assert_eq!(
        page.eval(
            "var images = document.images; document.body.appendChild(document.createElement('img')); \
             document.body.appendChild(document.createElement('embed')); document.body.appendChild(document.createElement('script')); \
             [images === document.images, images.length, document.embeds.length, document.plugins.length, document.scripts.length].join()"
        ),
        "true,1,1,1,1"
    );
    assert_eq!(
        page.eval(
            "var m = document.createElement('map'); m.appendChild(document.createElement('div')).appendChild(document.createElement('area')); \
             [m.areas.length, m.areas === m.areas].join()"
        ),
        "1,true"
    );
    // A `<body>`'s window event handlers are its window's.
    assert_eq!(
        page.eval("var f = () => 1; document.body.onhashchange = f; [onhashchange === f, document.body.onpopstate].join()"),
        "true,"
    );
}
