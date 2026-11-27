//! A child frame's place in its tab's session history (Blink's
//! `UniqueNameHelper`, which Chromium's renderer runs; the browser runs it
//! here, over its frame tree): its name if no other frame in the tab has
//! it, else a path of its ancestors' and its own position in its parent,
//! which a new document's iframe at the same place gets again. A frame
//! script made has a random one, which no frame of another document gets:
//! its place isn't restored.

use crate::frame_tree::FrameTree;
use protocol::FrameToken;

const FRAME_PATH_PREFIX: &str = "<!--framePath /";
const SUFFIX: &str = "-->";
const DYNAMIC_FRAME_MARKER: &str = "<!--dynamicFrame";

/// Longer names are hashed (most are shorter, Blink's
/// `kMaxRequestedNameSize`): a page may name a frame as long as it likes,
/// and its name is in every entry of the session history.
const MAX_REQUESTED_NAME_SIZE: usize = 80;

/// The unique name of a new child frame of `parent` named `name`, made by
/// script running if `created_by_script`
/// (`UniqueNameHelper::GenerateNameForNewChildFrame`).
pub(crate) fn for_new_child(
    tree: &FrameTree,
    parent: FrameToken,
    name: &str,
    created_by_script: bool,
) -> String {
    if created_by_script {
        let mut bytes = [0u8; 16];
        #[expect(
            clippy::expect_used,
            reason = "the operating system's random numbers, which every platform machopo runs \
                      on gives (Chromium's `base::RandBytes` aborts too without them)"
        )]
        getrandom::fill(&mut bytes).expect("the operating system gives random numbers");
        return format!("{DYNAMIC_FRAME_MARKER}{}{SUFFIX}", FrameToken(bytes));
    }
    let hashed;
    let name = if name.len() > MAX_REQUESTED_NAME_SIZE {
        hashed = frame_hash(name);
        hashed.as_str()
    } else {
        name
    };
    if !name.is_empty() && name != "_blank" && is_unique(tree, name) {
        return name.to_owned();
    }
    let candidate = candidate(tree, parent);
    if is_unique(tree, &candidate) {
        return candidate;
    }
    // Deterministic, so that it is the same as the page loads again.
    let prefix = format!("{candidate}{}/", frame_position(tree, parent));
    #[expect(
        clippy::expect_used,
        reason = "the tree's frames are fewer than u32::MAX: one of the numbers is no other's"
    )]
    (0u32..)
        .map(|retry| format!("{prefix}{retry}{SUFFIX}"))
        .find(|candidate| is_unique(tree, candidate))
        .expect("finitely many frames have the others")
}

/// Whether no frame of the tab has `name` (`IsCandidateUnique`).
fn is_unique(tree: &FrameTree, name: &str) -> bool {
    !tree.unique_names().any(|other| other == name)
}

/// `<!--frameHash…-->` of a name too long to keep. Blink's is SHA-256;
/// this one is 64 bits, which two names of a page share hardly ever, and
/// then the second takes a path all the same (`is_unique`).
fn frame_hash(name: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut hasher);
    format!("<!--frameHash{:016X}{SUFFIX}", hasher.finish())
}

/// `<!--framePath //<ancestors>/<!--frameN-->-->` (`GenerateCandidate`):
/// the unique names of `parent` and up, to the first that is a path
/// (whose path it extends) or the main frame's, and how many child frames
/// `parent` has.
fn candidate(tree: &FrameTree, parent: FrameToken) -> String {
    let mut ancestors = Vec::new();
    let mut at = Some(parent);
    while let Some(frame) = at {
        let name = tree.unique_name(frame).unwrap_or_default();
        ancestors.push(name);
        if is_frame_path(name) {
            break;
        }
        at = tree.parent(frame);
    }
    ancestors.reverse();
    let mut joined: Vec<&str> = ancestors;
    if let Some(first) = joined.first_mut()
        && let Some(path) = frame_path(first)
    {
        *first = path;
    }
    format!(
        "{FRAME_PATH_PREFIX}{}/<!--frame{}{SUFFIX}{SUFFIX}",
        joined.join("/"),
        tree.children(parent).len()
    )
}

/// `<!--framePosition-N-…`: the new frame's position in `parent` (as its
/// last child), then `parent`'s in its parent, up to the main frame
/// (`GenerateFramePosition`).
fn frame_position(tree: &FrameTree, parent: FrameToken) -> String {
    let mut position = String::from("<!--framePosition");
    let mut child: Option<FrameToken> = None;
    let mut at = Some(parent);
    while let Some(frame) = at {
        let children = tree.children(frame);
        let index = child.map_or(children.len(), |child| {
            children
                .iter()
                .position(|known| *known == child)
                .unwrap_or(children.len())
        });
        position.push('-');
        position.push_str(&index.to_string());
        child = Some(frame);
        at = tree.parent(frame);
    }
    position
}

fn is_frame_path(name: &str) -> bool {
    frame_path(name).is_some()
}

/// The path a frame path's name is of (`<!--framePath <path>-->`), if it
/// is one.
fn frame_path(name: &str) -> Option<&str> {
    name.strip_prefix(FRAME_PATH_PREFIX)?
        .strip_suffix(SUFFIX)
        .filter(|path| !path.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(n: u8) -> FrameToken {
        FrameToken([n; 16])
    }

    fn add(tree: &mut FrameTree, parent: u8, frame: u8, name: &str) -> String {
        tree.add_child(token(parent), token(frame), name, false)
            .unwrap();
        tree.unique_name(token(frame)).unwrap().to_owned()
    }

    /// A frame's name if it is no other's (not `_blank`), else the path
    /// of its ancestors' names and its position, as Blink makes them; a
    /// path taken already gets the frame's position all the way up, and a
    /// number.
    #[test]
    fn a_frame_is_its_name_or_its_path() {
        let mut tree = FrameTree::new(token(0));
        assert_eq!(tree.unique_name(token(0)), Some(""));
        assert_eq!(add(&mut tree, 0, 1, "a"), "a");
        assert_eq!(
            add(&mut tree, 0, 2, "a"),
            "<!--framePath //<!--frame1-->-->"
        );
        assert_eq!(
            add(&mut tree, 0, 3, "_blank"),
            "<!--framePath //<!--frame2-->-->"
        );
        assert_eq!(
            add(&mut tree, 1, 4, ""),
            "<!--framePath //a/<!--frame0-->-->"
        );
        assert_eq!(
            add(&mut tree, 2, 5, ""),
            "<!--framePath //<!--frame1-->/<!--frame0-->-->"
        );
        // The second child of 0 gone, the next has its path's number again.
        tree.remove(token(2)).unwrap();
        assert_eq!(
            add(&mut tree, 0, 6, ""),
            "<!--framePath //<!--frame2-->-->\
             <!--framePosition-2/0-->"
        );
        assert_eq!(add(&mut tree, 0, 7, ""), "<!--framePath //<!--frame3-->-->");
    }

    /// A long name is hashed, the same each time; one script made is
    /// random, another each time.
    #[test]
    fn long_names_are_hashed_and_scripts_frames_random() {
        let mut tree = FrameTree::new(token(0));
        let long = "x".repeat(MAX_REQUESTED_NAME_SIZE + 1);
        let hashed = add(&mut tree, 0, 1, &long);
        assert!(hashed.starts_with("<!--frameHash"), "{hashed}");
        assert_eq!(hashed, frame_hash(&long));
        assert_eq!(
            add(&mut tree, 0, 2, &"x".repeat(MAX_REQUESTED_NAME_SIZE)).len(),
            80
        );
        tree.add_child(token(0), token(3), "a", true).unwrap();
        tree.add_child(token(0), token(4), "a", true).unwrap();
        let (three, four) = (
            tree.unique_name(token(3)).unwrap(),
            tree.unique_name(token(4)).unwrap(),
        );
        assert!(
            three.starts_with(DYNAMIC_FRAME_MARKER) && three.ends_with(SUFFIX),
            "{three}"
        );
        assert_ne!(three, four);
    }
}
