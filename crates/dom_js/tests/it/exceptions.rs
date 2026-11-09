//! `DOMException`: what the DOM throws (its name, code and message, a
//! stack, an `Error`), and its interface: a constructor of a message and a
//! name, its constants, its attributes on its prototype. `QuotaExceededError`,
//! its child, and the structured clone of both.

use crate::env::Env;

/// A `DOMException` the DOM throws: of the realm's `DOMException` (made as
/// it is thrown, before script reads the interface object), an `Error`.
#[test]
fn the_dom_throws_dom_exceptions() {
    let mut env = Env::new();
    assert_eq!(
        env.eval(
            "var d = machopoDom.createDocument(); var e = d.createElement('p'); \
             try { e.appendChild(d); } catch (x) { var caught = x; } \
             [Object.getPrototypeOf(Object.getPrototypeOf(caught)) === Error.prototype, \
              caught instanceof DOMException, caught instanceof Error, caught.constructor === DOMException, \
              caught.name, caught.code, JSON.stringify(caught.message), typeof caught.stack].join()"
        ),
        "true,true,true,true,HierarchyRequestError,3,\"\",string"
    );
    assert_eq!(
        env.eval("try { d.createElement('1a'); } catch (x) { [x.name, x.code].join(); }"),
        "InvalidCharacterError,5"
    );
    assert_eq!(
        env.eval(
            "try { e.removeChild(d.createElement('q')); } catch (x) { [x.name, x.code].join(); }"
        ),
        "NotFoundError,8"
    );
}

#[test]
fn dom_exception_is_webidls() {
    let mut env = Env::new();
    let cases = [
        (
            "var x = new DOMException('m', 'NotFoundError'); [x.name, x.message, x.code].join()",
            "NotFoundError,m,8",
        ),
        (
            "var x = new DOMException(); [x.name, JSON.stringify(x.message), x.code].join()",
            "Error,\"\",0",
        ),
        ("new DOMException('m', 'Mine').code", "0"),
        ("new DOMException('m', 'QuotaExceededError').code", "22"),
        (
            "DOMException.NOT_FOUND_ERR === 8 && DOMException.prototype.DATA_CLONE_ERR === 25",
            "true",
        ),
        ("DOMException.length", "0"),
        (
            "Object.getOwnPropertyDescriptor(DOMException, 'prototype').writable",
            "false",
        ),
        (
            "Object.getPrototypeOf(DOMException.prototype) === Error.prototype",
            "true",
        ),
        (
            "String(new DOMException('went', 'AbortError'))",
            "AbortError: went",
        ),
        (
            "class Mine extends DOMException {}; new Mine('m', 'SyntaxError').code",
            "12",
        ),
        ("Object.keys(globalThis).includes('DOMException')", "false"),
        (
            "Object.prototype.toString.call(new DOMException())",
            "[object DOMException]",
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(env.eval(source), expected, "{source}");
    }
    assert!(
        env.eval("DOMException('m')")
            .starts_with("Exception: TypeError: Failed to construct 'DOMException'")
    );
    assert!(
        env.eval("Object.getOwnPropertyDescriptor(DOMException.prototype, 'name').get.call({})")
            .starts_with("Exception: TypeError: Illegal invocation")
    );
}

/// `QuotaExceededError` (WebIDL's): a `DOMException` named so, of code 22,
/// with its quota and the amount requested (null unless given; as Chrome
/// 152, no `RangeError` for a negative one); what the DOM throws for that
/// name (`getRandomValues` of more than 65536 bytes).
/// `Error.isError`: true for a `DOMException` of either kind, made by
/// script or thrown by the DOM (WebIDL gives one [[ErrorData]]); false for
/// the DOM's other objects and for an object made to look like one.
#[test]
fn a_dom_exception_is_an_error() {
    let mut env = Env::new();
    assert_eq!(
        env.eval(
            "var d = machopoDom.createDocument(); \
             try { d.createElement('1a'); } catch (x) { var thrown = x; } \
             [Error.isError(new DOMException()), Error.isError(new QuotaExceededError()), Error.isError(thrown), \
              Error.isError(d), Error.isError(Object.create(DOMException.prototype))].join()"
        ),
        "true,true,true,false,false"
    );
}

#[test]
fn quota_exceeded_error_is_webidls() {
    let mut env = Env::window("https://example.test/");
    let cases = [
        (
            "var q = new QuotaExceededError('full', { quota: 5, requested: 7 }); \
             [q instanceof DOMException, q instanceof Error, q.name, q.message, q.code, q.quota, q.requested, \
              Object.getPrototypeOf(QuotaExceededError) === DOMException, \
              Object.getOwnPropertyNames(QuotaExceededError.prototype).join('|'), \
              String(q), Object.prototype.toString.call(q)].join()",
            "true,true,QuotaExceededError,full,22,5,7,true,quota|requested|constructor,QuotaExceededError: full,[object QuotaExceededError]",
        ),
        (
            "var plain = new QuotaExceededError(); \
             [plain.message === '', plain.quota, plain.requested, new QuotaExceededError('m', { quota: -1, requested: 0 }).quota].join()",
            "true,,,-1",
        ),
        (
            "var error = f => { try { f(); return 'none'; } catch (e) { return e.name; } }; \
             [error(() => QuotaExceededError()), error(() => new QuotaExceededError('m', { quota: NaN })), \
              error(() => new QuotaExceededError('m', 1)), \
              error(() => Object.getOwnPropertyDescriptor(QuotaExceededError.prototype, 'quota').get.call(new DOMException()))].join()",
            "TypeError,TypeError,TypeError,TypeError",
        ),
        (
            "try { crypto.getRandomValues(new Uint8Array(65537)); } catch (e) { var thrown = e; } \
             [thrown instanceof QuotaExceededError, thrown.name, thrown.quota, thrown.requested, typeof thrown.stack].join()",
            "true,QuotaExceededError,,,string",
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(env.eval(source), expected, "{source}");
    }
}

/// A `DOMException` of either kind is `[Serializable]`: cloned as a new one
/// of the realm (its name and message; a `QuotaExceededError`'s quota and
/// amount requested too); another platform object, or a plain object with
/// `DOMException.prototype`, isn't one.
#[test]
fn dom_exceptions_are_cloned() {
    let mut env = Env::window("https://example.test/");
    let cases = [
        (
            "var d = new DOMException('gone', 'AbortError'); var c = structuredClone(d); \
             [c !== d, c instanceof DOMException, c.name, c.message, c.code].join()",
            "true,true,AbortError,gone,20",
        ),
        (
            "var q = structuredClone(new QuotaExceededError('full', { requested: 3 })); \
             [q instanceof QuotaExceededError, q.message, q.quota, q.requested].join()",
            "true,full,,3",
        ),
        (
            "var error = f => { try { f(); return 'none'; } catch (e) { return e.name; } }; \
             var fake = Object.create(DOMException.prototype); \
             [error(() => structuredClone(new Event('x'))), structuredClone(fake) instanceof DOMException].join()",
            "DataCloneError,false",
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(env.eval(source), expected, "{source}");
    }
}
