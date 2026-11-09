//! SVG's elements from script: each one's interface by its name (one
//! with no interface of its own an `SVGElement`), `SVGElement`'s members
//! (`className`, `ownerSVGElement`, `viewportElement`) and those it shares
//! with `HTMLElement`.

use std::time::Instant;

use dom_api::{ClassicScript, EngineSettings, ScriptEngine};
use dom_js::V8Engine;

/// What `source` pushed to `log`, joined with ` / `.
fn run(source: &str) -> String {
    let mut engine = V8Engine::new(EngineSettings { web_tests: true });
    engine
        .create_window(engine.top_frame(), "https://example.test/", Instant::now())
        .expect("a window");
    let run = |engine: &mut V8Engine, source: &str| {
        engine.run_classic_script(
            engine.top_frame(),
            &ClassicScript {
                source,
                url: "https://example.test/",
                base_url: None,
                line: 1,
                muted: false,
                use_credentials: false,
            },
        );
    };
    run(&mut engine, "var log = [];");
    run(&mut engine, source);
    run(
        &mut engine,
        "testRunner.reportHarnessResults(log.join(' / '))",
    );
    engine.take_harness_results().expect("reported")
}

#[test]
fn interfaces_by_name() {
    assert_eq!(
        run("var svg = 'http://www.w3.org/2000/svg';
             var make = name => document.createElementNS(svg, name);
             var rect = make('rect'), g = make('g'), other = make('frobnicate');
             log.push(String(rect), rect instanceof SVGRectElement, rect instanceof SVGGeometryElement,
                      rect instanceof SVGGraphicsElement, rect instanceof SVGElement, rect instanceof Element,
                      rect instanceof HTMLElement);
             log.push(String(g), g instanceof SVGGeometryElement, String(other),
                      String(make('linearGradient')), String(make('feBlend')), String(make('use')),
                      String(document.createElementNS('http://www.w3.org/1998/Math/MathML', 'math')));
             log.push(typeof SVGGeometryElement, Object.getPrototypeOf(SVGRectElement.prototype) === SVGGeometryElement.prototype);"),
        "[object SVGRectElement] / true / true / true / true / true / false / \
         [object SVGGElement] / false / [object SVGElement] / [object SVGLinearGradientElement] / \
         [object SVGFEBlendElement] / [object SVGUseElement] / [object Element] / function / true"
    );
}

/// `className`: the same `SVGAnimatedString` each time, its `baseVal` the
/// `class` attribute (set by setting it), `animVal` the same.
/// `ownerSVGElement`: the nearest `<svg>`, none for an outermost one;
/// `viewportElement`: the nearest `<svg>`, `<symbol>` or `<image>`. And
/// `HTMLElement`'s `style`, `dataset`, `tabIndex` (-1, but 0 for an
/// `<a>`, with a link or not, as Blink's `SVGAElement`).
#[test]
fn svg_element_members() {
    assert_eq!(
        run("document.appendChild(document.createElement('html')).appendChild(document.createElement('body'));
             document.body.innerHTML = '<svg id=outer><symbol id=sym><g id=g class=\"a b\"><rect id=r /></g></symbol>\
                 <svg id=inner><circle id=c /></svg></svg>';
             var $ = id => document.getElementById(id);
             var g = $('g');
             log.push(String(g.className), g.className === g.className, g.className.baseVal, g.className.animVal);
             g.className.baseVal = 'c';
             log.push(g.getAttribute('class'), g.className.animVal, $('r').className.baseVal === '');
             g.className = 'ignored';
             log.push(g.getAttribute('class'));
             log.push(String($('outer').ownerSVGElement), $('r').ownerSVGElement.id, $('c').ownerSVGElement.id,
                      $('inner').ownerSVGElement.id, $('r').viewportElement.id, $('c').viewportElement.id,
                      String($('outer').viewportElement));
             g.style.fill = 'red'; g.dataset.k = 'v';
             log.push(g.getAttribute('style'), g.getAttribute('data-k'), g.tabIndex);
             var a = document.createElementNS('http://www.w3.org/2000/svg', 'a');
             log.push(a.tabIndex);
             a.setAttribute('href', '#x');
             log.push(a.tabIndex, $('r').tabIndex);"),
        "[object SVGAnimatedString] / true / a b / a b / c / c / true / c / \
         null / outer / inner / outer / sym / inner / null / fill: red; / v / -1 / 0 / 0 / -1"
    );
}

/// HTML's `GlobalEventHandlers` on an SVG element: its content attribute
/// compiled, its IDL attribute set.
#[test]
fn event_handlers() {
    assert_eq!(
        run(
            "var g = document.createElementNS('http://www.w3.org/2000/svg', 'g');
             g.setAttribute('onclick', 'log.push(\"attribute\")');
             log.push(typeof g.onclick);
             g.onclick();
             g.onclick = () => log.push('property');
             g.dispatchEvent(new Event('click'));
             log.push(g.onclick === null, SVGElement.prototype.hasOwnProperty('onclick'));"
        ),
        "function / attribute / property / false / true"
    );
}
