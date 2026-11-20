//! HTTP cookies (Chromium's `//net/cookies`, RFC 6265bis): a `Set-Cookie`
//! line taken apart ([`ParsedCookie`]), the cookie it makes for the URL
//! that set it ([`CanonicalCookie`]), and the jar that keeps them and says
//! which a request sends ([`CookieMonster`]). Where Chromium's behavior (its
//! features as they are on by default) and the draft differ, Chromium's,
//! said where.
//!
//! The network context's loads use them through its jar (`CookieJar`),
//! which works out each hop's SameSite context from the request's trusted
//! params and keeps to the cookie policy. Partition keys, persistence and
//! how many cookies a domain may have are still to come.

mod canonical_cookie;
mod cookie_constants;
mod cookie_inclusion_status;
mod cookie_jar;
mod cookie_monster;
mod cookie_options;
mod cookie_util;
mod parsed_cookie;
mod time;

pub use canonical_cookie::{CanonicalCookie, MAX_EXPIRY_DAYS};
pub use cookie_constants::{CookieSameSite, CookieSourceScheme, CookieSourceType};
pub use cookie_inclusion_status::{CookieInclusionStatus, ExclusionReason};
pub(crate) use cookie_jar::CookieJar;
pub use cookie_monster::CookieMonster;
pub use cookie_options::{CookieOptions, LAX_ALLOW_UNSAFE_MAX_AGE_SECONDS, SameSiteCookieContext};
pub use cookie_util::parse_cookie_expiration_time;
pub use parsed_cookie::{
    MAX_COOKIE_ATTRIBUTE_VALUE_SIZE, MAX_COOKIE_NAME_PLUS_VALUE_SIZE, ParsedCookie,
};
pub use time::Time;
