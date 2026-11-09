//! Selection painting in a whole browser, against Chrome 152: each page
//! in Ahem (every glyph a 1em square) selects something on load, and each
//! case checks the colors along a few rows of pixels, from x 0 to 160, as
//! Chrome draws them: the x each run of a color starts at, and the color.

use std::path::PathBuf;

use gfx::Size;
use protocol::input::{InputEvent, KeyboardEvent, Modifiers};
use shell::headless;

/// What each page starts with: Ahem at 20px, blue text, `$(id)` and `sel`.
const HEAD: &str = r#"<style>html,body{margin:0}body{font:20px/1 Ahem;color:rgb(0,0,255)}p{margin:0}</style><script>const $=id=>document.getElementById(id);const sel=getSelection();</script>"#;

/// A page (after [`HEAD`]), and the colors along some rows: a row's y and
/// its runs, Chrome 152's.
type Case<'a> = (&'a str, &'a str, &'a [(usize, &'a str)]);

/// Renders each case's page and checks its rows, all of them before
/// failing.
fn check(cases: &[Case<'_>]) {
    check_after(cases, &[]);
}

/// [`check`], once the page has handled `input` (and drawn what it asked
/// for meanwhile).
fn check_after(cases: &[Case<'_>], input: &[InputEvent]) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-selection-painting");
    std::fs::create_dir_all(&dir).unwrap();
    let mut wrong = Vec::new();
    for (name, html, rows) in cases {
        let path = dir.join(format!("{name}.html"));
        std::fs::write(&path, format!("{HEAD}{html}")).unwrap();
        let url = net::file_url(&path).expect("an absolute path");
        let frame = headless::render_after_input(&url, Size::new(300, 200), 1.0, true, input)
            .expect("the page renders");
        let width = frame.size.width as usize;
        for &(y, want) in *rows {
            let mut runs: Vec<String> = Vec::new();
            let mut last = "";
            for x in 0..160 {
                let at = (y * width + x) * 4;
                let color = color_name(&frame.pixels[at..at + 3]);
                if color != last {
                    runs.push(format!("{x} {color}"));
                    last = color;
                }
            }
            let got = runs.join(", ");
            if got != want {
                wrong.push(format!("{name}, y {y}:\n  got  {got}\n  want {want}"));
            }
        }
    }
    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
}

/// A key down and up, with `modifiers`, typing `text`.
fn press(key: &str, key_code: u16, text: &str, modifiers: Modifiers) -> [InputEvent; 2] {
    let event = |down| {
        InputEvent::Key(KeyboardEvent {
            down,
            key: key.to_owned(),
            code: key.to_owned(),
            key_code,
            text: if down { text.to_owned() } else { String::new() },
            location: 0,
            repeat: false,
            modifiers,
        })
    };
    [event(true), event(false)]
}

fn color_name(rgb: &[u8]) -> &'static str {
    match rgb {
        [255, 255, 255] => "white",
        [0, 0, 0] => "black",
        [255, 0, 0] => "red",
        [0, 128, 0] => "green",
        [0, 0, 255] => "blue",
        [255, 0, 255] => "magenta",
        [255, 128, 0] => "orange",
        [255, 127, 127] => "#ff7f7f",
        // Half green over half yellow: Chrome blends it to 63, machopo to 64.
        [127, 191, 63 | 64] => "#7fbf3f",
        // The active highlight (`rgba(0, 65, 198, 0.8)` over white), and
        // the inactive one and its text.
        [51, 103, 209] => "highlight",
        [200, 200, 200] => "gray",
        [50, 50, 50] => "dark",
        _ => "other",
    }
}

/// The highlight's colors: the default ones (active), its `color` and
/// `background-color` with the other unset (the text's color stays, no
/// background), transparent, translucent, `currentcolor`; only the part
/// selected.
#[test]
fn the_highlights_colors() {
    check(&[
        (
            "colors-default",
            r#"<div id=d style='white-space:pre'>X X</div><script>sel.selectAllChildren($('d'))</script>"#,
            &[(5, "0 white, 20 highlight, 40 white")],
        ),
        (
            "colors-color",
            r#"<style>::selection{color:rgb(0,128,0)}</style><div id=d style='white-space:pre'>X X</div><script>sel.selectAllChildren($('d'))</script>"#,
            &[(5, "0 green, 20 white, 40 green, 60 white")],
        ),
        (
            "colors-background",
            r#"<style>::selection{background:rgb(255,0,0)}</style><div id=d style='white-space:pre'>X X</div><script>sel.selectAllChildren($('d'))</script>"#,
            &[(5, "0 blue, 20 red, 40 blue, 60 white")],
        ),
        (
            "colors-both",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><div id=d style='white-space:pre'>X X</div><script>sel.selectAllChildren($('d'))</script>"#,
            &[(5, "0 green, 20 red, 40 green, 60 white")],
        ),
        (
            "colors-transparent",
            r#"<style>::selection{background:transparent}</style><div id=d style='white-space:pre'>X X</div><script>sel.selectAllChildren($('d'))</script>"#,
            &[(5, "0 blue, 20 white, 40 blue, 60 white")],
        ),
        (
            "colors-translucent",
            r#"<style>::selection{background:rgba(255,0,0,0.5)}</style><div id=d style='white-space:pre'>X X</div><script>sel.selectAllChildren($('d'))</script>"#,
            &[(5, "0 blue, 20 #ff7f7f, 40 blue, 60 white")],
        ),
        (
            "colors-partial",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><div id=d>XXXX</div><script>sel.setBaseAndExtent($('d').firstChild,1,$('d').firstChild,3)</script>"#,
            &[(5, "0 blue, 20 green, 60 blue, 80 white")],
        ),
        (
            "colors-currentcolor",
            r#"<style>::selection{color:rgb(0,128,0);background:currentcolor}</style><div id=d style='white-space:pre'>X X</div><script>sel.selectAllChildren($('d'))</script>"#,
            &[(5, "0 green, 60 white")],
        ),
    ]);
}

/// The default colors, both, unless a `::selection` rule for the element
/// or an ancestor declares `color` or `background-color` (an empty rule,
/// one with only a decoration, a shadow, an invalid color or a custom
/// property keeps them; `inherit` and `transparent` don't); then the one
/// not declared is the text's color, or no background.
#[test]
fn the_default_colors_unless_a_rule_has_colors() {
    check(&[
        (
            "paired-empty",
            r#"<style>::selection{}</style><div id=d style='white-space:pre'>X X</div><script>sel.selectAllChildren($('d'))</script>"#,
            &[(5, "0 white, 20 highlight, 40 white")],
        ),
        (
            "paired-decoration",
            r#"<style>::selection{text-decoration:underline}</style><div id=d style='white-space:pre'>X X</div><script>sel.selectAllChildren($('d'))</script>"#,
            &[(5, "0 white, 20 highlight, 40 white")],
        ),
        (
            "paired-invalid-color",
            r#"<style>::selection{color:foo}</style><div id=d style='white-space:pre'>X X</div><script>sel.selectAllChildren($('d'))</script>"#,
            &[(5, "0 white, 20 highlight, 40 white")],
        ),
        (
            "paired-border-radius",
            r#"<style>::selection{border-radius:5px;padding:3px}</style><div id=d style='white-space:pre'>X X</div><script>sel.selectAllChildren($('d'))</script>"#,
            &[(5, "0 white, 20 highlight, 40 white")],
        ),
        (
            "paired-shadow",
            r#"<style>::selection{text-shadow:2px 2px rgb(255,0,255)}</style><div id=d style='white-space:pre'>X X</div><script>sel.selectAllChildren($('d'))</script>"#,
            &[
                (
                    5,
                    "0 white, 20 magenta, 22 highlight, 40 white, 60 magenta, 62 white",
                ),
                (21, "0 white, 2 magenta, 22 white, 42 magenta, 62 white"),
            ],
        ),
        (
            "paired-parent-color-child-deco",
            r#"<style>#p::selection{color:rgb(0,128,0)} #d::selection{text-decoration:underline}</style><div id=p><div id=d style='white-space:pre'>X X</div></div><script>sel.selectAllChildren($('d'))</script>"#,
            &[(5, "0 green, 20 white, 40 green, 60 white")],
        ),
        (
            "paired-parent-bg-child-empty",
            r#"<style>#p::selection{background-color:rgb(255,0,0)} #d::selection{}</style><div id=p><div id=d style='white-space:pre'>X X</div></div><script>sel.selectAllChildren($('d'))</script>"#,
            &[(5, "0 blue, 20 red, 40 blue, 60 white")],
        ),
        (
            "paired-custom-prop",
            r#"<style>::selection{--x:1}</style><div id=d style='white-space:pre'>X X</div><script>sel.selectAllChildren($('d'))</script>"#,
            &[(5, "0 white, 20 highlight, 40 white")],
        ),
        (
            "paired-transparent-color",
            r#"<style>::selection{color:transparent}</style><div id=d style='white-space:pre'>X X</div><script>sel.selectAllChildren($('d'))</script>"#,
            &[(5, "0 white")],
        ),
        (
            "paired-inherit-color",
            r#"<style>::selection{color:inherit}</style><div id=d style='white-space:pre'>X X</div><script>sel.selectAllChildren($('d'))</script>"#,
            &[(5, "0 blue, 20 white, 40 blue, 60 white")],
        ),
    ]);
}

/// A selection over a shadow host is over its shadow tree, in flat tree
/// order: the tree's text and what its slot shows are drawn selected, and
/// the ends of their lines.
#[test]
fn through_a_shadow_tree() {
    check(&[(
        "shadow-host",
        r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><div id=d><p>X</p><div id=h></div><p>X</p></div><script>const r=$('h').attachShadow({mode:'open'});r.innerHTML="<style>p{margin:0}::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><p style='white-space:pre'>X X</p><slot></slot>";$('h').innerHTML="<span>XX</span>";sel.selectAllChildren($('d'))</script>"#,
        &[
            (5, "0 green, 20 red, 40 white"),
            (25, "0 green, 20 red, 40 green, 60 red, 80 white"),
            (45, "0 green, 40 red, 60 white"),
            (65, "0 green, 20 white"),
        ],
    )]);
}

/// The background is as tall as the line box or the font's box, whichever
/// reaches further.
#[test]
fn as_tall_as_the_line_or_the_font() {
    check(&[
        (
            "height-tall-line",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><div id=d style='line-height:40px;white-space:pre'>X X</div><script>sel.selectAllChildren($('d'))</script>"#,
            &[
                (1, "0 red, 60 white"),
                (15, "0 green, 20 red, 40 green, 60 white"),
                (25, "0 green, 20 red, 40 green, 60 white"),
                (38, "0 red, 60 white"),
                (41, "0 white"),
            ],
        ),
        (
            "height-short-line",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><div style='margin-top:20px'><div id=d style='line-height:10px;white-space:pre'>X X</div></div><script>sel.selectAllChildren($('d'))</script>"#,
            &[
                (14, "0 white"),
                (16, "0 green, 20 red, 40 green, 60 white"),
                (34, "0 green, 20 red, 40 green, 60 white"),
                (36, "0 white"),
            ],
        ),
    ]);
}

/// A space's box after a line's text, when the selection is over its end:
/// a `<br>` or a kept newline in it (in the `<br>`'s font, on an empty
/// line too), a soft wrap it goes past (a hanging space isn't the line's
/// text), the end of a block it goes on past to something laid out (not
/// select-all's last line, not before hidden text, before an image). It is
/// after the text, before an inline element's padding, which isn't drawn
/// selected; none after an atomic inline ending the line, but a `<br>`'s
/// or a newline's after it.
#[test]
fn the_end_of_a_line() {
    check(&[
        (
            "end-block-goes-on",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><p id=a>XX</p><p id=b>XX</p><script>sel.setBaseAndExtent($('a').firstChild,0,$('b').firstChild,1)</script>"#,
            &[
                (5, "0 green, 40 red, 60 white"),
                (25, "0 green, 20 blue, 40 white"),
            ],
        ),
        (
            "end-block-ends-in-text",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><p id=a>XX</p><p id=b>XX</p><script>sel.setBaseAndExtent($('a').firstChild,0,$('a').firstChild,2)</script>"#,
            &[(5, "0 green, 40 white"), (25, "0 blue, 40 white")],
        ),
        (
            "end-block-ends-after",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><p id=a>XX</p><p id=b>XX</p><script>sel.setBaseAndExtent($('a').firstChild,0,document.body,2)</script>"#,
            &[(5, "0 green, 40 red, 60 white"), (25, "0 green, 40 white")],
        ),
        (
            "end-select-all",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><p id=a>XX</p>
<p id=b>XX</p>
<script>sel.selectAllChildren(document.body)</script>"#,
            &[(5, "0 green, 40 red, 60 white"), (25, "0 green, 40 white")],
        ),
        (
            "end-to-hidden",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><p id=a>XX</p><p id=b style='display:none'>XX</p><script>sel.setBaseAndExtent($('a').firstChild,0,$('b').firstChild,1)</script>"#,
            &[(5, "0 green, 40 white")],
        ),
        (
            "end-to-image",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><p id=a>XX</p><p id=b><img style='width:10px;height:10px;vertical-align:top'></p><script>sel.setBaseAndExtent($('a').firstChild,0,$('b'),1)</script>"#,
            &[(5, "0 green, 40 red, 60 white")],
        ),
        (
            "end-collapsed-spaces",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><p id=a>XX   </p><p id=b>XX</p><script>sel.setBaseAndExtent($('a').firstChild,0,document.body,1)</script>"#,
            &[(5, "0 green, 40 white")],
        ),
        (
            "end-wrap",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><div id=d style='width:50px'>XX XX</div><script>sel.selectAllChildren($('d'))</script>"#,
            &[(5, "0 green, 40 red, 60 white"), (25, "0 green, 40 white")],
        ),
        (
            "end-wrap-hanging-space-only",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><div id=d style='width:50px'>XX XX</div><script>sel.setBaseAndExtent($('d').firstChild,2,$('d').firstChild,5)</script>"#,
            &[(5, "0 blue, 40 white"), (25, "0 green, 40 white")],
        ),
        (
            "end-wrap-to-space",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><div id=d style='width:50px'>XX XX</div><script>sel.setBaseAndExtent($('d').firstChild,0,$('d').firstChild,3)</script>"#,
            &[(5, "0 green, 40 red, 60 white"), (25, "0 blue, 40 white")],
        ),
        (
            "end-break-all",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><div id=d style='width:50px;word-break:break-all'>XXXX</div><script>sel.selectAllChildren($('d'))</script>"#,
            &[(5, "0 green, 40 red, 60 white"), (25, "0 green, 40 white")],
        ),
        (
            "end-br",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><div id=d>XX<br><br>X</div><script>sel.selectAllChildren($('d'))</script>"#,
            &[
                (5, "0 green, 40 red, 60 white"),
                (25, "0 red, 20 white"),
                (45, "0 green, 20 white"),
            ],
        ),
        (
            "end-br-only",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><div id=d>XX<br>X</div><script>sel.setBaseAndExtent($('d'),1,$('d'),2)</script>"#,
            &[(5, "0 blue, 40 red, 60 white"), (25, "0 blue, 20 white")],
        ),
        (
            "end-before-br",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><div id=d>XX<br>X</div><script>sel.setBaseAndExtent($('d').firstChild,0,$('d'),1)</script>"#,
            &[(5, "0 green, 40 white"), (25, "0 blue, 20 white")],
        ),
        (
            "end-br-font",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><div id=d>XX<br style='font-size:10px'>X</div><script>sel.selectAllChildren($('d'))</script>"#,
            &[(5, "0 green, 40 red, 50 white"), (25, "0 green, 20 white")],
        ),
        (
            "end-newline",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><pre id=d style='margin:0;font:inherit'>XX
X</pre><script>sel.setBaseAndExtent($('d').firstChild,2,$('d').firstChild,3)</script>"#,
            &[(5, "0 blue, 40 red, 60 white"), (25, "0 blue, 20 white")],
        ),
        (
            "end-newline-not",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><pre id=d style='margin:0;font:inherit'>XX
X</pre><script>sel.setBaseAndExtent($('d').firstChild,0,$('d').firstChild,2)</script>"#,
            &[(5, "0 green, 40 white"), (25, "0 blue, 20 white")],
        ),
        (
            "end-padding",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><p id=a>X<span style='padding:0 20px'>X</span></p><p id=b>X</p><script>sel.setBaseAndExtent($('a').firstChild,0,$('b').firstChild,1)</script>"#,
            &[(5, "0 green, 20 white, 40 green, 60 red, 80 white")],
        ),
        (
            "inline-padding",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><div id=d>X<span style='padding:0 20px'>X</span>X</div><script>sel.selectAllChildren($('d'))</script>"#,
            &[(
                5,
                "0 green, 20 white, 40 green, 60 white, 80 green, 100 white",
            )],
        ),
        (
            "atomic-last",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><p id=a>X<span style='display:inline-block;width:20px;height:20px;vertical-align:top;background:rgb(255,0,255)'></span></p><p id=b>X</p><script>sel.setBaseAndExtent($('a').firstChild,0,$('b').firstChild,1)</script>"#,
            &[
                (5, "0 green, 20 magenta, 40 white"),
                (25, "0 green, 20 white"),
            ],
        ),
        (
            "atomic-then-br",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><div id=d>X<span style='display:inline-block;width:20px;height:20px;vertical-align:top;background:rgb(255,0,255)'></span><br>X</div><script>sel.selectAllChildren($('d'))</script>"#,
            &[
                (5, "0 green, 20 magenta, 40 red, 60 white"),
                (25, "0 green, 20 white"),
            ],
        ),
        (
            "atomic-wrap",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><div id=d style='width:50px'>X<span style='display:inline-block;width:20px;height:20px;vertical-align:top;background:rgb(255,0,255)'></span> X</div><script>sel.selectAllChildren($('d'))</script>"#,
            &[
                (5, "0 green, 20 magenta, 40 white"),
                (25, "0 green, 20 white"),
            ],
        ),
        (
            "atomic-newline",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><div id=d style='white-space:pre'>X<span style='display:inline-block;width:20px;height:20px;vertical-align:top;background:rgb(255,0,255)'></span>
X</div><script>sel.selectAllChildren($('d'))</script>"#,
            &[
                (5, "0 green, 20 magenta, 40 red, 60 white"),
                (25, "0 green, 20 white"),
            ],
        ),
    ]);
}

/// Text with `user-select: none` isn't drawn selected; a text control's
/// selection is drawn only while it has the focus, and then the document's
/// isn't; a frame without the focus draws its selection in the inactive
/// colors.
#[test]
fn what_isnt_drawn_selected() {
    check(&[
        (
            "user-select-none",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><div id=d>X<span style='user-select:none'>X</span>X</div><script>sel.selectAllChildren($('d'))</script>"#,
            &[(5, "0 green, 20 blue, 40 green, 60 white")],
        ),
        (
            "input-focused",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><input id=i value='X  X' style='font:inherit;border:0;padding:0;margin:0;width:100px;outline:none'><script>$('i').focus();$('i').setSelectionRange(1,3)</script>"#,
            &[(5, "0 black, 20 red, 60 black, 80 white")],
        ),
        (
            "input-not-focused",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><input id=i value='X  X' style='font:inherit;border:0;padding:0;margin:0;width:100px;outline:none'><script>$('i').setSelectionRange(1,3)</script>"#,
            &[(5, "0 black, 20 white, 60 black, 80 white")],
        ),
        (
            "input-and-document",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><input id=i value='X  X' style='font:inherit;border:0;padding:0;margin:0;width:100px;outline:none'><div id=d>XX</div><script>sel.selectAllChildren($('d'));$('i').focus();$('i').setSelectionRange(1,3)</script>"#,
            &[
                (5, "0 black, 20 red, 60 black, 80 white"),
                (25, "0 blue, 40 white"),
            ],
        ),
        (
            "frame-not-focused",
            r#"<iframe style='border:0;width:200px;height:40px;display:block' srcdoc="<style>html,body{margin:0}body{font:20px/1 Ahem;color:rgb(0,0,255)}</style><div id=d style='white-space:pre'>X X</div><script>getSelection().selectAllChildren(document.getElementById('d'))</script>"></iframe>"#,
            &[(5, "0 dark, 20 gray, 40 dark, 60 white")],
        ),
    ]);
}

/// A selection changed after the page is drawn paints again what it
/// leaves and what it takes: text right in a block, after a `<br>` (and
/// the `<br>`'s box), in an inline element, in a text control that loses
/// the focus.
#[test]
fn a_selection_changed_later() {
    check(&[
        (
            "later-moved",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><div id=d>XX<br>X<p id=p>X</p>X<span id=s>X</span></div><script>document.documentElement.className='reftest-wait';sel.selectAllChildren($('d'));requestAnimationFrame(()=>requestAnimationFrame(()=>setTimeout(()=>{sel.selectAllChildren($('s'));requestAnimationFrame(()=>requestAnimationFrame(()=>document.documentElement.className=''))},0)))</script>"#,
            &[
                (5, "0 blue, 40 white"),
                (25, "0 blue, 20 white"),
                (45, "0 blue, 20 white"),
                (65, "0 blue, 20 green, 40 white"),
            ],
        ),
        (
            "later-into-a-block",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><div id=d>XX<br>X<p id=p>X</p>X<span id=s>X</span></div><script>document.documentElement.className='reftest-wait';sel.setBaseAndExtent($('d').firstChild,0,$('d').firstChild,1);requestAnimationFrame(()=>requestAnimationFrame(()=>setTimeout(()=>{sel.setBaseAndExtent($('d').firstChild,1,$('p').firstChild,1);requestAnimationFrame(()=>requestAnimationFrame(()=>document.documentElement.className=''))},0)))</script>"#,
            &[
                (5, "0 blue, 20 green, 40 red, 60 white"),
                (25, "0 green, 20 red, 40 white"),
                (45, "0 green, 20 white"),
                (65, "0 blue, 40 white"),
            ],
        ),
        (
            "later-cleared",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><div id=d>XX<br>X<p id=p>X</p>X<span id=s>X</span></div><script>document.documentElement.className='reftest-wait';sel.selectAllChildren($('d'));requestAnimationFrame(()=>requestAnimationFrame(()=>setTimeout(()=>{sel.removeAllRanges();requestAnimationFrame(()=>requestAnimationFrame(()=>document.documentElement.className=''))},0)))</script>"#,
            &[
                (5, "0 blue, 40 white"),
                (25, "0 blue, 20 white"),
                (45, "0 blue, 20 white"),
                (65, "0 blue, 40 white"),
            ],
        ),
        (
            "later-input-blurred",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><input id=i value='X  X' style='font:inherit;border:0;padding:0;margin:0;width:100px;outline:none'><script>document.documentElement.className='reftest-wait';$('i').focus();$('i').setSelectionRange(1,3);requestAnimationFrame(()=>requestAnimationFrame(()=>setTimeout(()=>{$('i').blur();requestAnimationFrame(()=>requestAnimationFrame(()=>document.documentElement.className=''))},0)))</script>"#,
            &[(5, "0 black, 20 white, 60 black, 80 white")],
        ),
    ]);
}

/// Decorations and shadows: the text's decorations are drawn again in the
/// highlight's color over the selected part, the highlight's own in
/// theirs, and not in theirs too (a translucent highlight shows none
/// under it); the text's shadows are covered by the background there,
/// the highlight's drawn over the text after it (moved with the text,
/// past the selection).
#[test]
fn decorations_and_shadows() {
    check(&[
        (
            "deco-underline",
            r#"<style>body{font:40px/60px Ahem;white-space:pre}</style><style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><div id=d style='text-decoration:underline rgb(255,0,255)'>X  </div><script>sel.setBaseAndExtent($('d').firstChild,0,$('d').firstChild,1)</script>"#,
            &[
                (12, "0 green, 40 white"),
                (46, "0 green, 40 magenta, 120 white"),
            ],
        ),
        (
            "deco-line-through",
            r#"<style>body{font:40px/60px Ahem;white-space:pre}</style><style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><div id=d style='text-decoration:line-through'>   </div><script>sel.setBaseAndExtent($('d').firstChild,0,$('d').firstChild,1)</script>"#,
            &[(30, "0 green, 40 blue, 120 white")],
        ),
        (
            "deco-own",
            r#"<style>body{font:40px/60px Ahem;white-space:pre}</style><style>::selection{color:rgb(0,128,0);background:rgb(255,0,0);text-decoration:line-through rgb(255,128,0)}</style><div id=d>X  </div><script>sel.setBaseAndExtent($('d').firstChild,0,$('d').firstChild,1)</script>"#,
            &[(12, "0 green, 40 white"), (30, "0 orange, 40 white")],
        ),
        (
            "deco-own-currentcolor",
            r#"<style>body{font:40px/60px Ahem;white-space:pre}</style><style>::selection{color:rgb(0,128,0);background:rgb(255,0,0);text-decoration:underline}</style><div id=d>   </div><script>sel.setBaseAndExtent($('d').firstChild,0,$('d').firstChild,1)</script>"#,
            &[(46, "0 green, 40 white")],
        ),
        (
            "shadow-element",
            r#"<style>body{font:40px/60px Ahem;white-space:pre}</style><style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><div id=d style='text-shadow:5px 5px rgb(255,0,255)'>X X</div><script>sel.setBaseAndExtent($('d').firstChild,0,$('d').firstChild,1)</script>"#,
            &[
                (12, "0 green, 40 white, 80 blue, 120 white"),
                (
                    46,
                    "0 green, 40 magenta, 45 white, 80 blue, 120 magenta, 125 white",
                ),
                (52, "0 red, 40 magenta, 45 white, 85 magenta, 125 white"),
            ],
        ),
        (
            "shadow-own",
            r#"<style>body{font:40px/60px Ahem;white-space:pre}</style><style>::selection{color:rgb(0,128,0);background:rgb(255,0,0);text-shadow:5px 5px rgb(255,128,0)}</style><div id=d style='text-shadow:5px 5px rgb(255,0,255)'>XX </div><script>sel.setBaseAndExtent($('d').firstChild,0,$('d').firstChild,1)</script>"#,
            &[
                (12, "0 green, 40 blue, 80 white"),
                (46, "0 green, 40 orange, 45 blue, 80 magenta, 85 white"),
                (52, "0 red, 5 orange, 45 magenta, 85 white"),
            ],
        ),
        (
            "deco-translucent-line-through",
            r#"<style>body{font:40px/60px Ahem;white-space:pre}</style><style>::selection{color:rgba(0,128,0,0.5);background:rgba(255,255,0,0.5)}</style><div id=d style='text-decoration:line-through rgb(255,0,255)'>   </div><script>sel.setBaseAndExtent($('d').firstChild,0,$('d').firstChild,1)</script>"#,
            &[(30, "0 #7fbf3f, 40 magenta, 120 white")],
        ),
        (
            "deco-translucent-underline",
            r#"<style>body{font:40px/60px Ahem;white-space:pre}</style><style>::selection{color:rgba(0,128,0,0.5);background:rgba(255,255,0,0.5)}</style><div id=d style='text-decoration:underline rgb(255,0,255);text-decoration-skip-ink:none'>   </div><script>sel.setBaseAndExtent($('d').firstChild,0,$('d').firstChild,1)</script>"#,
            &[(46, "0 #7fbf3f, 40 magenta, 120 white")],
        ),
    ]);
}

/// A selection a `keydown` listener changes, the DOM as it was (Escape,
/// which activates nothing): what it leaves and takes are painted again (a
/// main frame for it, its text found again).
#[test]
fn a_key_changes_the_selection_of_the_document() {
    check_after(
        &[(
            "key-document",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><div id=a style='white-space:pre'>X X</div><div id=b style='white-space:pre'>X X</div><script>sel.selectAllChildren($('a'));addEventListener('keydown',()=>sel.selectAllChildren($('b')))</script>"#,
            &[
                (5, "0 blue, 20 white, 40 blue, 60 white"),
                (25, "0 green, 20 red, 40 green, 60 white"),
            ],
        )],
        &press("Escape", 27, "", Modifiers::NONE),
    );
}

/// A focused text control's selection the user extends with Shift+→, its
/// value as it was: painted again.
#[test]
fn a_key_changes_the_selection_of_the_text_control() {
    check_after(
        &[(
            "key-text-control",
            r#"<style>::selection{color:rgb(0,128,0);background:rgb(255,0,0)}</style><input id=i value='X  X' style='font:inherit;border:0;padding:0;margin:0;width:100px;outline:none'><script>$('i').focus();$('i').setSelectionRange(1,1)</script>"#,
            &[(5, "0 black, 20 red, 60 black, 80 white")],
        )],
        &[
            press("ArrowRight", 39, "", SHIFT),
            press("ArrowRight", 39, "", SHIFT),
        ]
        .concat(),
    );
}

const SHIFT: Modifiers = Modifiers {
    shift: true,
    ..Modifiers::NONE
};

/// What each page draws that isn't white (its text is white): where, how
/// many pixels and in which colors (the first four), as Chrome 152 draws it
/// (`--disable-blink-features=CaretBlinking` didn't stop it blinking:
/// several screenshots, together).
fn check_carets(cases: &[(&str, &str, &str)]) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-carets");
    std::fs::create_dir_all(&dir).unwrap();
    let mut wrong = Vec::new();
    for (name, html, want) in cases {
        let path = dir.join(format!("{name}.html"));
        std::fs::write(&path, format!("{CARET_HEAD}{html}")).unwrap();
        let url = net::file_url(&path).expect("an absolute path");
        let frame =
            headless::render(&url, Size::new(300, 200), 1.0, true).expect("the page renders");
        let width = frame.size.width as usize;
        let mut drawn = Vec::new();
        for y in 0..100 {
            for x in 0..160 {
                let at = (y * width + x) * 4;
                let rgb = [frame.pixels[at], frame.pixels[at + 1], frame.pixels[at + 2]];
                if rgb != [255, 255, 255] {
                    drawn.push((x, y, rgb));
                }
            }
        }
        let got = match (
            drawn.iter().map(|d| d.0).min(),
            drawn.iter().map(|d| d.0).max(),
        ) {
            (Some(left), Some(right)) => {
                let top = drawn.iter().map(|d| d.1).min().unwrap_or(0);
                let bottom = drawn.iter().map(|d| d.1).max().unwrap_or(0);
                let mut colors: Vec<[u8; 3]> = drawn.iter().map(|d| d.2).collect();
                colors.sort();
                colors.dedup();
                let colors: Vec<String> = colors
                    .iter()
                    .take(4)
                    .map(|[r, g, b]| format!("({r}, {g}, {b})"))
                    .collect();
                format!(
                    "x {left}..{right} y {top}..{bottom} n {} colors [{}]",
                    drawn.len(),
                    colors.join(", ")
                )
            }
            _ => "nothing".to_string(),
        };
        if got != *want {
            wrong.push(format!("{name}:\n  got  {got}\n  want {want}"));
        }
    }
    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
}

/// What the caret pages start with: Ahem at 20px, white text.
const CARET_HEAD: &str = r#"<style>html,body{margin:0}body{font:20px/1 Ahem;color:white}</style><script>const $=id=>document.getElementById(id);const sel=getSelection();</script>"#;

/// The caret, against Chrome 152: in a focused text control or editing
/// host with the selection collapsed, 1px wide and as tall as the font
/// where it is (centered in a taller line), at the offset (downstream: a
/// soft wrap's next line, after a `<br>` or a kept newline the next line;
/// after the last `<br>`, which starts no line, that `<br>`'s line),
/// in `caret-color` (`auto` the text's); in an empty one where its line
/// would start (`text-align`, `text-indent`, its font). None for a range,
/// without the focus, in a read-only control, `transparent`, or where one
/// can't type.
#[test]
fn carets() {
    check_carets(CARETS);
}

const CARETS: &[(&str, &str, &str)] = &[
    (
        "input-end",
        r#"<input id=i value='XX' style='font:inherit;border:0;padding:0;margin:0;outline:none;color:white;caret-color:rgb(255,0,0);width:100px'><script>$('i').focus()</script>"#,
        "x 0..0 y 0..19 n 20 colors [(255, 0, 0)]",
    ),
    (
        "input-start",
        r#"<input id=i value='XX' style='font:inherit;border:0;padding:0;margin:0;outline:none;color:white;caret-color:rgb(255,0,0);width:100px'><script>$('i').focus();$('i').setSelectionRange(0,0)</script>"#,
        "x 0..0 y 0..19 n 20 colors [(255, 0, 0)]",
    ),
    (
        "input-1",
        r#"<input id=i value='XX' style='font:inherit;border:0;padding:0;margin:0;outline:none;color:white;caret-color:rgb(255,0,0);width:100px'><script>$('i').focus();$('i').setSelectionRange(1,1)</script>"#,
        "x 20..20 y 0..19 n 20 colors [(255, 0, 0)]",
    ),
    (
        "input-empty",
        r#"<input id=i value='' style='font:inherit;border:0;padding:0;margin:0;outline:none;color:white;caret-color:rgb(255,0,0);width:100px'><script>$('i').focus()</script>"#,
        "x 0..0 y 0..19 n 20 colors [(255, 0, 0)]",
    ),
    (
        "input-tall-line",
        r#"<input id=i value='XX' style='font:inherit;border:0;padding:0;margin:0;outline:none;color:white;caret-color:rgb(255,0,0);width:100px;line-height:40px;height:40px'><script>$('i').focus();$('i').setSelectionRange(1,1)</script>"#,
        "x 20..20 y 10..29 n 20 colors [(255, 0, 0)]",
    ),
    (
        "input-padding",
        r#"<input id=i value='XX' style='font:inherit;border:0;padding:0;margin:0;outline:none;color:white;caret-color:rgb(255,0,0);width:100px;padding:5px 7px'><script>$('i').focus();$('i').setSelectionRange(1,1)</script>"#,
        "x 27..27 y 5..24 n 20 colors [(255, 0, 0)]",
    ),
    (
        "input-range",
        r#"<input id=i value='XX' style='font:inherit;border:0;padding:0;margin:0;outline:none;color:white;caret-color:rgb(255,0,0);width:100px'><script>$('i').focus();$('i').setSelectionRange(0,1)</script>"#,
        "nothing",
    ),
    (
        "input-not-focused",
        r#"<input id=i value='XX' style='font:inherit;border:0;padding:0;margin:0;outline:none;color:white;caret-color:rgb(255,0,0);width:100px'><script>$('i').setSelectionRange(1,1)</script>"#,
        "nothing",
    ),
    (
        "input-readonly",
        r#"<input id=i readonly value='XX' style='font:inherit;border:0;padding:0;margin:0;outline:none;color:white;caret-color:rgb(255,0,0);width:100px'><script>$('i').focus();$('i').setSelectionRange(1,1)</script>"#,
        "nothing",
    ),
    (
        "input-auto-color",
        r#"<input id=i value='XX' style='font:inherit;border:0;padding:0;margin:0;outline:none;color:white;caret-color:rgb(255,0,0);width:100px;caret-color:auto;color:rgb(0,0,255)'><script>$('i').focus();$('i').setSelectionRange(0,0)</script>"#,
        "x 0..39 y 0..19 n 800 colors [(0, 0, 255)]",
    ),
    (
        "input-transparent",
        r#"<input id=i value='XX' style='font:inherit;border:0;padding:0;margin:0;outline:none;color:white;caret-color:rgb(255,0,0);width:100px;caret-color:transparent'><script>$('i').focus();$('i').setSelectionRange(1,1)</script>"#,
        "nothing",
    ),
    (
        "textarea-2nd-line",
        r#"<textarea id=t style='font:inherit;border:0;padding:0;margin:0;outline:none;color:white;caret-color:rgb(255,0,0);width:100px;height:60px;resize:none'>XX
XX</textarea><script>$('t').focus();$('t').setSelectionRange(4,4)</script>"#,
        "x 20..20 y 20..39 n 20 colors [(255, 0, 0)]",
    ),
    (
        "textarea-after-newline",
        r#"<textarea id=t style='font:inherit;border:0;padding:0;margin:0;outline:none;color:white;caret-color:rgb(255,0,0);width:100px;height:60px;resize:none'>XX
</textarea><script>$('t').focus();$('t').setSelectionRange(3,3)</script>"#,
        "x 0..0 y 20..39 n 20 colors [(255, 0, 0)]",
    ),
    (
        "ce-text",
        r#"<div id=e contenteditable style='caret-color:rgb(255,0,0);outline:none'>XX</div><script>$('e').focus();sel.collapse($('e').firstChild,1)</script>"#,
        "x 20..20 y 0..19 n 20 colors [(255, 0, 0)]",
    ),
    (
        "ce-tall-line",
        r#"<div id=e contenteditable style='caret-color:rgb(255,0,0);outline:none;line-height:40px'>XX</div><script>$('e').focus();sel.collapse($('e').firstChild,1)</script>"#,
        "x 20..20 y 10..29 n 20 colors [(255, 0, 0)]",
    ),
    (
        "ce-empty",
        r#"<div id=e contenteditable style='caret-color:rgb(255,0,0);outline:none;padding:3px 9px'></div><script>$('e').focus()</script>"#,
        "x 9..9 y 3..22 n 20 colors [(255, 0, 0)]",
    ),
    (
        "ce-span-font",
        r#"<div id=e contenteditable style='caret-color:rgb(255,0,0);outline:none'>X<span id=s style='font-size:40px'>X</span></div><script>$('e').focus();sel.collapse($('s').firstChild,1)</script>"#,
        "x 60..60 y 0..39 n 40 colors [(255, 0, 0)]",
    ),
    (
        "ce-wrap",
        r#"<div id=e contenteditable style='caret-color:rgb(255,0,0);outline:none;width:50px'>XX XX</div><script>$('e').focus();sel.collapse($('e').firstChild,3)</script>"#,
        "x 0..0 y 20..39 n 20 colors [(255, 0, 0)]",
    ),
    (
        "ce-end-of-wrapped-line",
        r#"<div id=e contenteditable style='caret-color:rgb(255,0,0);outline:none;width:50px'>XX XX</div><script>$('e').focus();sel.collapse($('e').firstChild,2)</script>"#,
        "x 40..40 y 0..19 n 20 colors [(255, 0, 0)]",
    ),
    (
        "ce-after-br",
        r#"<div id=e contenteditable style='caret-color:rgb(255,0,0);outline:none'>XX<br>X</div><script>$('e').focus();sel.collapse($('e'),2)</script>"#,
        "x 0..0 y 20..39 n 20 colors [(255, 0, 0)]",
    ),
    (
        "ce-center",
        r#"<div id=e contenteditable style='caret-color:rgb(255,0,0);outline:none;text-align:center;width:100px'></div><script>$('e').focus()</script>"#,
        "x 50..50 y 0..19 n 20 colors [(255, 0, 0)]",
    ),
    (
        "ce-not-focused",
        r#"<div id=e contenteditable style='caret-color:rgb(255,0,0);outline:none'>XX</div><button id=b style='position:absolute;left:200px'>b</button><script>sel.collapse($('e').firstChild,1);$('b').focus();sel.collapse($('e').firstChild,1)</script>"#,
        "nothing",
    ),
    (
        "non-editable",
        r#"<div id=e tabindex=0 style='caret-color:rgb(255,0,0);outline:none'>XX</div><script>$('e').focus();sel.collapse($('e').firstChild,1)</script>"#,
        "nothing",
    ),
    (
        "ce-empty-tall",
        r#"<div id=e contenteditable style='caret-color:rgb(255,0,0);outline:none;line-height:40px'></div><script>$('e').focus();document.title=$('e').offsetHeight</script>"#,
        "x 0..0 y 10..29 n 20 colors [(255, 0, 0)]",
    ),
    (
        "input-empty-tall",
        r#"<input id=i value='' style='font:inherit;border:0;padding:0;margin:0;outline:none;color:white;caret-color:rgb(255,0,0);width:100px;line-height:40px;height:40px'><script>$('i').focus()</script>"#,
        "x 0..0 y 10..29 n 20 colors [(255, 0, 0)]",
    ),
    (
        "ce-empty-right",
        r#"<div id=e contenteditable style='caret-color:rgb(255,0,0);outline:none;text-align:right;width:100px'></div><script>$('e').focus()</script>"#,
        "x 99..99 y 0..19 n 20 colors [(255, 0, 0)]",
    ),
    (
        "ce-empty-big-font",
        r#"<div id=e contenteditable style='caret-color:rgb(255,0,0);outline:none;font-size:40px'></div><script>$('e').focus()</script>"#,
        "x 0..0 y 0..39 n 40 colors [(255, 0, 0)]",
    ),
    (
        "ce-empty-indent",
        r#"<div id=e contenteditable style='caret-color:rgb(255,0,0);outline:none;text-indent:30px'></div><script>$('e').focus()</script>"#,
        "x 30..30 y 0..19 n 20 colors [(255, 0, 0)]",
    ),
    (
        "ce-anonymous",
        r#"<div id=e contenteditable style='caret-color:rgb(255,0,0);outline:none'>XX<p style='margin:0'>Y</p></div><script>$('e').focus();sel.collapse($('e').firstChild,1)</script>"#,
        "x 20..20 y 0..19 n 20 colors [(255, 0, 0)]",
    ),
    (
        "ce-empty-line",
        r#"<div id=e contenteditable style='caret-color:rgb(255,0,0);outline:none'>XX<br><br>X</div><script>$('e').focus();sel.collapse($('e'),2)</script>"#,
        "x 0..0 y 20..39 n 20 colors [(255, 0, 0)]",
    ),
    (
        "ce-after-last-br",
        r#"<div id=e contenteditable style='caret-color:rgb(255,0,0);outline:none'>XX<br><br></div><script>$('e').focus();sel.collapse($('e'),3)</script>"#,
        "x 0..0 y 20..39 n 20 colors [(255, 0, 0)]",
    ),
    (
        "ce-before-br",
        r#"<div id=e contenteditable style='caret-color:rgb(255,0,0);outline:none'>XX<br>X</div><script>$('e').focus();sel.collapse($('e'),1)</script>"#,
        "x 40..40 y 0..19 n 20 colors [(255, 0, 0)]",
    ),
    (
        "textarea-after-newline-tall",
        r#"<textarea id=t style='font:inherit;border:0;padding:0;margin:0;outline:none;color:white;caret-color:rgb(255,0,0);width:100px;height:90px;resize:none;line-height:40px'>XX
</textarea><script>$('t').focus();$('t').setSelectionRange(3,3)</script>"#,
        "x 0..0 y 50..69 n 20 colors [(255, 0, 0)]",
    ),
    (
        "ce-before-small-br",
        r#"<div id=e contenteditable style='caret-color:rgb(255,0,0);outline:none'>XX<br style='font-size:10px'>X</div><script>$('e').focus();sel.collapse($('e'),1)</script>"#,
        "x 40..40 y 0..19 n 20 colors [(255, 0, 0)]",
    ),
];
