//! Fetch's header list (https://fetch.spec.whatwg.org/#concept-header-list,
//! Blink's `FetchHeaderList`): names and values, in order, each a
//! `ByteString` (each character a byte); names match without regard to
//! ASCII case. The algorithms the DOM's `Headers` uses; what a guard
//! forbids is `network`'s.

/// A header list: names and values, in order.
pub type HeaderList = Vec<(String, String)>;

/// HTTP's `token` (a header name): one or more `tchar`s.
pub fn is_header_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(
                    b,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

/// Fetch's HTTP whitespace bytes.
fn is_http_whitespace(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | ' ')
}

/// Fetch's "normalize" a header value: leading and trailing HTTP
/// whitespace goes.
pub fn normalize(value: &str) -> &str {
    value.trim_matches(is_http_whitespace)
}

/// Fetch's header value (normalized): no NUL, LF or CR.
pub fn is_header_value(value: &str) -> bool {
    !value.contains(['\0', '\n', '\r'])
}

/// Fetch's "forbidden response-header name": `Set-Cookie`, `Set-Cookie2`.
pub fn is_forbidden_response_header_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("set-cookie") || name.eq_ignore_ascii_case("set-cookie2")
}

fn matches_name(entry: &(String, String), name: &str) -> bool {
    entry.0.eq_ignore_ascii_case(name)
}

/// A header list "contains" `name`.
pub fn contains(list: &HeaderList, name: &str) -> bool {
    list.iter().any(|entry| matches_name(entry, name))
}

/// "Get" `name` from a header list: its values joined with ", ", or `None`.
pub fn get(list: &HeaderList, name: &str) -> Option<String> {
    let values: Vec<&str> = list
        .iter()
        .filter(|entry| matches_name(entry, name))
        .map(|(_, value)| value.as_str())
        .collect();
    (!values.is_empty()).then(|| values.join(", "))
}

/// The values of `name`, each on its own (`getSetCookie()`'s).
pub fn values(list: &HeaderList, name: &str) -> Vec<String> {
    list.iter()
        .filter(|entry| matches_name(entry, name))
        .map(|(_, value)| value.clone())
        .collect()
}

/// "Append" to a header list: a name already in it takes the case it has
/// there.
pub fn append(list: &mut HeaderList, name: &str, value: &str) {
    let name = list
        .iter()
        .find(|entry| matches_name(entry, name))
        .map_or_else(|| name.to_owned(), |(first, _)| first.clone());
    list.push((name, value.to_owned()));
}

/// "Delete" `name` from a header list: every one of it.
pub fn delete(list: &mut HeaderList, name: &str) {
    list.retain(|entry| !matches_name(entry, name));
}

/// "Set" a header list's `name` to `value`: the first of it takes the
/// value, the others go; or it is appended.
pub fn set(list: &mut HeaderList, name: &str, value: &str) {
    match list.iter().position(|entry| matches_name(entry, name)) {
        Some(first) => {
            value.clone_into(&mut list[first].1);
            let mut index = 0;
            list.retain(|entry| {
                let keep = index <= first || !matches_name(entry, name);
                #[expect(
                    clippy::arithmetic_side_effects,
                    reason = "one per entry: fewer than the list's length"
                )]
                {
                    index += 1;
                }
                keep
            });
        }
        None => list.push((name.to_owned(), value.to_owned())),
    }
}

/// Fetch's "sort and combine": the names lowercased, in byte order, each
/// once with its values joined, but `set-cookie`, once per value.
pub fn sort_and_combine(list: &HeaderList) -> HeaderList {
    let mut names: Vec<String> = list
        .iter()
        .map(|(name, _)| name.to_ascii_lowercase())
        .collect();
    names.sort_unstable();
    names.dedup();
    let mut combined = Vec::new();
    for name in names {
        if name == "set-cookie" {
            for value in values(list, &name) {
                combined.push((name.clone(), value));
            }
        } else {
            let value = get(list, &name).expect("a name of the list");
            combined.push((name, value));
        }
    }
    combined
}

/// Fetch's "remove privileged no-CORS request-headers" (`Range`), after a
/// change to a `request-no-cors` `Headers`.
pub fn remove_privileged_no_cors_request_headers(list: &mut HeaderList) {
    list.retain(|(name, _)| !crate::network::is_privileged_no_cors_request_header_name(name));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(entries: &[(&str, &str)]) -> HeaderList {
        entries
            .iter()
            .map(|&(name, value)| (name.to_owned(), value.to_owned()))
            .collect()
    }

    #[test]
    fn the_lists_algorithms() {
        let mut headers = list(&[("Accept", "a"), ("X-B", "1")]);
        append(&mut headers, "accept", "b");
        assert_eq!(
            headers,
            list(&[("Accept", "a"), ("X-B", "1"), ("Accept", "b")]),
            "the first case"
        );
        assert_eq!(get(&headers, "ACCEPT").as_deref(), Some("a, b"));
        assert_eq!(get(&headers, "nope"), None);
        assert!(contains(&headers, "x-b"));
        set(&mut headers, "accept", "c");
        assert_eq!(headers, list(&[("Accept", "c"), ("X-B", "1")]));
        set(&mut headers, "Z", "z");
        delete(&mut headers, "x-b");
        assert_eq!(headers, list(&[("Accept", "c"), ("Z", "z")]));
        let cookies = list(&[
            ("Set-Cookie", "a=1"),
            ("b", "2"),
            ("set-cookie", "c=3"),
            ("B", "4"),
        ]);
        assert_eq!(
            sort_and_combine(&cookies),
            list(&[("b", "2, 4"), ("set-cookie", "a=1"), ("set-cookie", "c=3")])
        );
        assert_eq!(values(&cookies, "SET-COOKIE"), ["a=1", "c=3"]);
        assert_eq!(normalize(" \t a b \r\n"), "a b");
        assert!(is_header_name("X-Ok_~") && !is_header_name("bad name") && !is_header_name(""));
        assert!(is_header_value("a b") && !is_header_value("a\nb"));
        let mut ranged = list(&[("Range", "bytes=0-1"), ("Accept", "a")]);
        remove_privileged_no_cors_request_headers(&mut ranged);
        assert_eq!(ranged, list(&[("Accept", "a")]));
    }
}
