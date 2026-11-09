//! HTML's `srcset` (https://html.spec.whatwg.org/#parse-a-srcset-attribute)
//! and the choice among its candidates (Blink's `HTMLSrcsetParser`): the
//! parser follows HTML step by step; the choice is Blink's
//! `PickBestImageCandidate`, as HTML leaves it to the user agent.

/// What a candidate says about itself.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Descriptor {
    /// None: 1x.
    None,
    /// `w`: its width in image pixels.
    Width(u32),
    /// `x`: its pixel density.
    Density(f64),
}

/// An image candidate: its URL as written, and its descriptor.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub url: String,
    pub descriptor: Descriptor,
}

fn is_space(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\x0c' | '\r' | ' ')
}

/// HTML's "parse a srcset attribute": the candidates, in order, without
/// those whose descriptors are in error.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "`position` steps only past a char it read: at most `chars.len()`"
)]
pub fn parse(input: &str) -> Vec<Candidate> {
    let chars: Vec<char> = input.chars().collect();
    let mut position = 0;
    let mut candidates = Vec::new();
    loop {
        // Splitting loop: whitespace and commas.
        while position < chars.len() && (is_space(chars[position]) || chars[position] == ',') {
            position += 1;
        }
        if position >= chars.len() {
            return candidates;
        }
        let start = position;
        while position < chars.len() && !is_space(chars[position]) {
            position += 1;
        }
        let mut url: String = chars[start..position].iter().collect();
        let mut descriptors = Vec::new();
        if url.ends_with(',') {
            // Trailing commas end the candidate: no descriptors.
            url = url.trim_end_matches(',').to_owned();
        } else {
            descriptors = tokenize_descriptors(&chars, &mut position);
        }
        if let Some(descriptor) = parse_descriptors(&descriptors) {
            candidates.push(Candidate { url, descriptor });
        }
    }
}

/// HTML's descriptor tokenizer, from `position`, to the comma that ends the
/// candidate (consumed) or the end.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "`position` steps only past a char it read (at the end it returns): at most \
              `chars.len()`"
)]
fn tokenize_descriptors(chars: &[char], position: &mut usize) -> Vec<String> {
    #[derive(PartialEq)]
    enum State {
        InDescriptor,
        InParens,
        AfterDescriptor,
    }
    while *position < chars.len() && is_space(chars[*position]) {
        *position += 1;
    }
    let mut descriptors = Vec::new();
    let mut current = String::new();
    let mut state = State::InDescriptor;
    loop {
        let c = chars.get(*position).copied();
        match state {
            State::InDescriptor => match c {
                Some(c) if is_space(c) => {
                    if !current.is_empty() {
                        descriptors.push(std::mem::take(&mut current));
                        state = State::AfterDescriptor;
                    }
                }
                Some(',') => {
                    *position += 1;
                    if !current.is_empty() {
                        descriptors.push(current);
                    }
                    return descriptors;
                }
                Some('(') => {
                    current.push('(');
                    state = State::InParens;
                }
                None => {
                    if !current.is_empty() {
                        descriptors.push(current);
                    }
                    return descriptors;
                }
                Some(c) => current.push(c),
            },
            State::InParens => match c {
                Some(')') => {
                    current.push(')');
                    state = State::InDescriptor;
                }
                None => {
                    descriptors.push(current);
                    return descriptors;
                }
                Some(c) => current.push(c),
            },
            State::AfterDescriptor => match c {
                Some(c) if is_space(c) => {}
                None => return descriptors,
                Some(_) => {
                    state = State::InDescriptor;
                    // Consume it again, in descriptor.
                    continue;
                }
            },
        }
        *position += 1;
    }
}

/// HTML's descriptor parser: `None` for an error.
fn parse_descriptors(descriptors: &[String]) -> Option<Descriptor> {
    let (mut width, mut density, mut height) = (None, None, None);
    for descriptor in descriptors {
        // The last char is the kind.
        let (last, _) = descriptor.char_indices().next_back()?;
        let (value, kind) = descriptor.split_at(last);
        match kind {
            "w" => {
                if width.is_some() || density.is_some() {
                    return None;
                }
                let value = valid_non_negative_integer(value)?;
                if value == 0 {
                    return None;
                }
                width = Some(value);
            }
            "x" => {
                if width.is_some() || density.is_some() || height.is_some() {
                    return None;
                }
                let value = valid_float(value)?;
                if value < 0.0 {
                    return None;
                }
                density = Some(value);
            }
            "h" => {
                if height.is_some() || density.is_some() {
                    return None;
                }
                let value = valid_non_negative_integer(value)?;
                if value == 0 {
                    return None;
                }
                height = Some(value);
            }
            _ => return None,
        }
    }
    // A height only goes with a width (for the future: it is ignored).
    if height.is_some() && width.is_none() {
        return None;
    }
    Some(match (width, density) {
        (Some(width), _) => Descriptor::Width(width),
        (None, Some(density)) => Descriptor::Density(density),
        (None, None) => Descriptor::None,
    })
}

/// A "valid non-negative integer": ASCII digits only.
fn valid_non_negative_integer(text: &str) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // Past `u32`: as large as it gets.
    Some(text.parse().unwrap_or(u32::MAX))
}

/// A "valid floating-point number": an optional `-`, digits and/or a full
/// stop and digits, an optional exponent; nothing else.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "`i` steps only past a byte `bytes.get` found: at most `bytes.len()`"
)]
fn valid_float(text: &str) -> Option<f64> {
    let bytes = text.as_bytes();
    let mut i = 0;
    if bytes.first() == Some(&b'-') {
        i += 1;
    }
    let digits = |i: &mut usize| {
        let start = *i;
        while bytes.get(*i).is_some_and(u8::is_ascii_digit) {
            *i += 1;
        }
        *i > start
    };
    let integer = digits(&mut i);
    let mut fraction = false;
    if bytes.get(i) == Some(&b'.') {
        i += 1;
        fraction = digits(&mut i);
        if !fraction {
            return None;
        }
    }
    if !integer && !fraction {
        return None;
    }
    if matches!(bytes.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(bytes.get(i), Some(b'-' | b'+')) {
            i += 1;
        }
        if !digits(&mut i) {
            return None;
        }
    }
    if i != bytes.len() {
        return None;
    }
    text.parse().ok()
}

/// Blink's `PickBestImageCandidate`: each candidate's density (a width's
/// over `source_size`, none 1x), sorted by it, one per density (the first;
/// with a width among them, `src` (`from_src`) doesn't count), then the
/// first at least `device_pixel_ratio` (or the one before, if
/// `device_pixel_ratio` is past the two's geometric mean, or under 1x and
/// over the lower), or else the densest; a denser one the document has
/// already (`cached`) wins over it. The URL as written and the density.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "`i` < `last` = `deduped.len() - 1` and `winner` < `deduped.len()`: one past \
              either is at most the length"
)]
pub fn pick(
    candidates: &[Candidate],
    src: Option<&str>,
    source_size: f32,
    device_pixel_ratio: f32,
    cached: impl Fn(&str) -> bool,
) -> Option<(String, f64)> {
    let mut sized: Vec<(f64, &str, bool)> = Vec::new();
    let mut has_width = false;
    for candidate in candidates {
        let density = match candidate.descriptor {
            Descriptor::None => 1.0,
            Descriptor::Density(density) => density,
            Descriptor::Width(width) => {
                has_width = true;
                f64::from(width) / f64::from(source_size)
            }
        };
        sized.push((density, &candidate.url, false));
    }
    if let Some(src) = src.filter(|src| !src.is_empty()) {
        sized.push((1.0, src, true));
    }
    sized.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut deduped: Vec<(f64, &str)> = Vec::new();
    let mut previous = None;
    for (density, url, from_src) in sized {
        if previous != Some(density) && !(has_width && from_src) {
            deduped.push((density, url));
        }
        previous = Some(density);
    }
    let dpr = f64::from(device_pixel_ratio);
    // None of them: no candidate.
    let mut winner = deduped.len().checked_sub(1)?;
    let last = winner;
    for i in 0..last {
        let next = deduped[i + 1].0;
        if next < dpr {
            continue;
        }
        let current = deduped[i].0;
        let geometric_mean = (current * next).sqrt();
        winner = if (dpr <= 1.0 && dpr > current) || dpr >= geometric_mean {
            i + 1
        } else {
            i
        };
        break;
    }
    // Blink's `AvoidDownloadIfHigherDensityResourceIsInCache`.
    if let Some(denser) = (winner + 1..deduped.len())
        .rev()
        .find(|&i| cached(deduped[i].1))
    {
        winner = denser;
    }
    let (density, url) = deduped[winner];
    Some((url.to_owned(), density))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(url: &str, descriptor: Descriptor) -> Candidate {
        Candidate {
            url: url.to_owned(),
            descriptor,
        }
    }

    #[test]
    fn srcset_parsing() {
        assert_eq!(
            parse(" a.png 1x, b.png 2x ,c.png 300w,d.png, e.png 1.5x"),
            [
                candidate("a.png", Descriptor::Density(1.0)),
                candidate("b.png", Descriptor::Density(2.0)),
                candidate("c.png", Descriptor::Width(300)),
                candidate("d.png", Descriptor::None),
                candidate("e.png", Descriptor::Density(1.5)),
            ]
        );
        // Errors drop the candidate: two descriptors of a kind, 0w, a
        // negative or malformed density, a height alone, an unknown one.
        assert_eq!(
            parse("a 1x 2x, b 0w, c -1x, d 1.x, e 10h, f 1q, g 10w 10h, h .5x, i 1e1x"),
            [
                candidate("g", Descriptor::Width(10)),
                candidate("h", Descriptor::Density(0.5)),
                candidate("i", Descriptor::Density(10.0)),
            ]
        );
        // A comma without a space after it is the URL's; trailing commas
        // end a URL; a comma inside parentheses doesn't end a descriptor.
        assert_eq!(
            parse("d.png,e.png 2x"),
            [candidate("d.png,e.png", Descriptor::Density(2.0))]
        );
        assert_eq!(
            parse("data:a,b,, x 1x"),
            [
                candidate("data:a,b", Descriptor::None),
                candidate("x", Descriptor::Density(1.0)),
            ]
        );
        assert_eq!(
            parse("a (1x, 2x) 3x, b"),
            [candidate("b", Descriptor::None)]
        );
        assert_eq!(parse(""), []);
        assert_eq!(parse(" , ,"), []);
    }

    #[test]
    fn picking() {
        let x = |url: &str, density| candidate(url, Descriptor::Density(density));
        let none = |_: &str| false;
        let set = [x("1", 1.0), x("2", 2.0), x("3", 3.0)];
        assert_eq!(pick(&set, None, 100.0, 1.0, none), Some(("1".into(), 1.0)));
        assert_eq!(pick(&set, None, 100.0, 2.0, none), Some(("2".into(), 2.0)));
        // 1.5: under the geometric mean of 1 and 2 (1.41...)? No: over it.
        assert_eq!(pick(&set, None, 100.0, 1.5, none), Some(("2".into(), 2.0)));
        assert_eq!(pick(&set, None, 100.0, 1.3, none), Some(("1".into(), 1.0)));
        assert_eq!(pick(&set, None, 100.0, 4.0, none), Some(("3".into(), 3.0)));
        // Under 1x, past the lowest: the next.
        let low = [x("half", 0.5), x("1", 1.0)];
        assert_eq!(pick(&low, None, 100.0, 0.75, none), Some(("1".into(), 1.0)));
        // Widths over the source size; `src` doesn't count among them.
        let widths = [
            candidate("200", Descriptor::Width(200)),
            candidate("400", Descriptor::Width(400)),
        ];
        assert_eq!(
            pick(&widths, Some("src"), 200.0, 1.0, none),
            Some(("200".into(), 1.0))
        );
        assert_eq!(
            pick(&widths, Some("src"), 200.0, 2.0, none),
            Some(("400".into(), 2.0))
        );
        // `src`, a 1x, would be the closest at 1x.
        assert_eq!(
            pick(&widths, Some("src"), 100.0, 1.0, none),
            Some(("200".into(), 2.0))
        );
        // `src` is 1x, after a 1x of `srcset`.
        assert_eq!(
            pick(&[x("2", 2.0)], Some("src"), 100.0, 1.0, none),
            Some(("src".into(), 1.0))
        );
        assert_eq!(
            pick(&[x("one", 1.0)], Some("src"), 100.0, 1.0, none),
            Some(("one".into(), 1.0))
        );
        // A denser one in the cache.
        assert_eq!(
            pick(&set, None, 100.0, 1.0, |url| url == "3"),
            Some(("3".into(), 3.0))
        );
        assert_eq!(pick(&[], None, 100.0, 1.0, none), None);
        assert_eq!(pick(&[], Some(""), 100.0, 1.0, none), None);
    }
}
