//! Editability from script: `contentEditable` reflecting the attribute's
//! state and setting it, `isContentEditable` (from the DOM: the same out
//! of the document and in `display: none`), and `:read-write` matching
//! editable elements.

use crate::env::Env;

fn page() -> Env {
    let mut env = Env::window("https://example.test/a/page.html");
    env.eval(
        "var html = document.appendChild(document.createElement('html')); \
         var body = html.appendChild(document.createElement('body')); \
         body.innerHTML = '<div id=ce contenteditable><span id=inner>x</span>\
           <b id=off contenteditable=false><i id=offi>y</i></b></div>\
           <div id=plain contenteditable=plaintext-only><i id=plaini>z</i></div>\
           <div id=bogus contenteditable=bogus><i id=bogusi>w</i></div>\
           <div contenteditable><svg id=svg><g id=g></g></svg><math id=math></math></div>'; \
         globalThis.d = document.createElement('div'); \
         globalThis.ids = ids => ids.map(id => document.getElementById(id)); \
         globalThis.error = f => { try { f(); return 'none'; } catch (e) { return e.name + ': ' + e.message; } }; 0",
    );
    env
}

/// The nearest HTML element whose `contenteditable` isn't inherit decides;
/// an `<svg>` and a `<math>` inside an editable element are editable, an
/// SVG element in them not (Chrome's `IsEditableOrEditingHost`).
#[test]
fn whether_an_element_is_editable() {
    let mut env = page();
    assert_eq!(
        env.eval(
            "JSON.stringify(ids(['ce', 'inner', 'off', 'offi', 'plain', 'plaini', 'bogus', 'bogusi'])
               .map(e => e.isContentEditable).concat(body.isContentEditable))"
        ),
        "[true,true,false,false,true,true,false,false,false]"
    );
    assert_eq!(
        env.eval(
            "const e = document.createElement('div'); e.contentEditable = 'true';
             const child = document.createElement('span'); e.append(child);
             const hidden = document.createElement('div'); hidden.contentEditable = 'true';
             hidden.style.display = 'none'; body.append(hidden);
             JSON.stringify([e.isContentEditable, child.isContentEditable, hidden.isContentEditable])"
        ),
        "[true,true,true]"
    );
    assert_eq!(
        env.eval(
            "JSON.stringify(ids(['svg', 'g', 'math', 'ce', 'inner', 'off', 'plain', 'bogus'])
               .map(e => e.matches(':read-write')))"
        ),
        "[true,false,true,true,true,false,true,false]"
    );
    // Its own `contenteditable` changing changes what is editable below.
    assert_eq!(
        env.eval(
            "const ce = document.getElementById('ce');
             ce.contentEditable = 'false';
             const after = [ce.isContentEditable, document.getElementById('inner').matches(':read-only')];
             ce.removeAttribute('contenteditable');
             JSON.stringify(after.concat(document.getElementById('inner').isContentEditable))"
        ),
        "[false,true,false]"
    );
}

/// `contentEditable` is the attribute's state as its keyword (ASCII
/// case-insensitively, empty true, anything else inherit); setting it
/// lowercases the keyword, `inherit` removes the attribute, anything else
/// is a `SyntaxError`.
#[test]
fn content_editable_reflects_the_state() {
    let mut env = page();
    for (value, expected) in [
        ("null", "inherit"),
        ("''", "true"),
        ("'true'", "true"),
        ("'TRUE'", "true"),
        ("'false'", "false"),
        ("'plaintext-only'", "plaintext-only"),
        ("'PlainText-Only'", "plaintext-only"),
        ("'inherit'", "inherit"),
        ("'x'", "inherit"),
    ] {
        assert_eq!(
            env.eval(&format!(
                "{value} === null ? d.removeAttribute('contenteditable')
                   : d.setAttribute('contenteditable', {value});
                 d.contentEditable"
            )),
            expected,
            "{value}"
        );
    }
    assert_eq!(
        env.eval(
            "const got = [];
             for (const value of ['True', 'FALSE', 'plaintext-only', 'INHERIT']) {
               d.contentEditable = value;
               got.push(d.getAttribute('contenteditable'));
             }
             JSON.stringify(got)"
        ),
        r#"["true","false","plaintext-only",null]"#
    );
    assert_eq!(
        env.eval(
            "error(() => d.contentEditable = 'x') + ' / ' + error(() => d.contentEditable = '')"
        ),
        "SyntaxError: The value provided ('x') is not one of 'true', 'false', 'plaintext-only', \
         or 'inherit'. / SyntaxError: The value provided ('') is not one of 'true', 'false', \
         'plaintext-only', or 'inherit'."
    );
}
