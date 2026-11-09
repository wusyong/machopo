//! Where a caret can first be in an editing host
//! (`first_caret_position`) reads each run's first cluster's start, worked
//! out alone (`TextFragment::first_cluster_start`): what the clusters say,
//! with none of the runs' clusters worked out (a host taking the focus
//! would work out every cluster of its text).

use layout::{BoxFragment, TextFragment};

use crate::page::{Page, find};

fn runs(fragment: &BoxFragment) -> Vec<&TextFragment> {
    let mut out: Vec<&TextFragment> = fragment.lines.iter().flat_map(|line| &line.runs).collect();
    for child in &fragment.children {
        out.extend(runs(&child.fragment));
    }
    out
}

/// White space that collapses away at the start, a combining mark, right
/// to left text, several lines: the first cluster's start alone is the
/// clusters' first's.
#[test]
fn a_runs_first_cluster_start_is_its_clusters_firsts() {
    let mut page = Page::build(
        "<div style='width: 80px'>   e\u{301}xyz <b>abc</b> \u{5e9}\u{5dc}\u{5d5}\u{5dd} \
         long words wrap here</div>",
    );
    let view = page.lay_out().view;
    let runs = runs(&view);
    assert!(runs.len() > 3, "several runs: {}", runs.len());
    for run in runs {
        let first = run.first_cluster_start();
        assert!(!run.clusters_made(), "worked out alone");
        assert_eq!(first, run.clusters().first().map(|cluster| cluster.start));
    }
}

/// The caret's first place in a host, before its first character that
/// didn't collapse away, found with no run's clusters worked out.
#[test]
fn the_carets_first_place_works_out_no_clusters() {
    let mut page = Page::build(
        "<div id=host contenteditable>  <p id=p>  one two</p><p>three four five</p></div>",
    );
    let view = page.lay_out().view;
    page.read(|cx, document| {
        let host = find(cx, document, "host");
        let (node, offset) = layout::first_caret_position(cx, &page.tree, &view, host);
        let p = find(cx, document, "p");
        assert!(
            cx.node_id(cx.parent(node).unwrap()) == cx.node_id(p),
            "in the first paragraph's text"
        );
        assert_eq!(offset, 2, "past the white space that collapsed away");
    });
    assert!(runs(&view).iter().all(|run| !run.clusters_made()));
}
