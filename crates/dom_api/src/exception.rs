//! What a member throws: WebIDL's `TypeError` (its arguments' conversion)
//! or a `DOMException` the spec's algorithm throws, by its name.

use std::fmt;

use dom::DomError;

/// The error a member's call throws.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiError {
    /// A `TypeError`, with its message.
    Type(String),
    /// A `RangeError`, with its message.
    Range(String),
    /// A `DOMException`.
    Dom(DomException),
}

impl ApiError {
    /// Its message, for a host that reports it as text.
    pub fn message(&self) -> String {
        match self {
            Self::Type(message) | Self::Range(message) => message.clone(),
            Self::Dom(exception) => exception.message.clone(),
        }
    }

    pub fn type_error(message: impl Into<String>) -> Self {
        Self::Type(message.into())
    }

    /// The `DOMException` `name`, with no message.
    pub fn dom(name: ExceptionName) -> Self {
        Self::Dom(DomException {
            name,
            message: String::new(),
        })
    }
}

impl From<DomError> for ApiError {
    fn from(error: DomError) -> Self {
        Self::dom(match error {
            DomError::HierarchyRequest => ExceptionName::HierarchyRequest,
            DomError::NotFound => ExceptionName::NotFound,
            DomError::InUseAttribute => ExceptionName::InUseAttribute,
        })
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Type(message) => write!(f, "TypeError: {message}"),
            Self::Range(message) => write!(f, "RangeError: {message}"),
            Self::Dom(exception) => write!(f, "{}: {}", exception.name.name(), exception.message),
        }
    }
}

impl std::error::Error for ApiError {}

/// A `DOMException`: its name and message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomException {
    pub name: ExceptionName,
    pub message: String,
}

/// The `DOMException` names the API throws (WebIDL's table of them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExceptionName {
    HierarchyRequest,
    NotFound,
    InvalidCharacter,
    InvalidState,
    InvalidAccess,
    Syntax,
    Namespace,
    NoModificationAllowed,
    NotSupported,
    NotAllowed,
    IndexSize,
    Encoding,
    Security,
    DataClone,
    InUseAttribute,
    WrongDocument,
    InvalidNodeType,
    Abort,
    TypeMismatch,
    QuotaExceeded,
    InvalidModification,
    Timeout,
}

impl ExceptionName {
    /// Its `name`.
    pub fn name(self) -> &'static str {
        match self {
            Self::HierarchyRequest => "HierarchyRequestError",
            Self::NotFound => "NotFoundError",
            Self::InvalidCharacter => "InvalidCharacterError",
            Self::InvalidState => "InvalidStateError",
            Self::InvalidAccess => "InvalidAccessError",
            Self::Syntax => "SyntaxError",
            Self::Namespace => "NamespaceError",
            Self::NoModificationAllowed => "NoModificationAllowedError",
            Self::NotSupported => "NotSupportedError",
            Self::NotAllowed => "NotAllowedError",
            Self::IndexSize => "IndexSizeError",
            Self::Encoding => "EncodingError",
            Self::Security => "SecurityError",
            Self::DataClone => "DataCloneError",
            Self::InUseAttribute => "InUseAttributeError",
            Self::WrongDocument => "WrongDocumentError",
            Self::InvalidNodeType => "InvalidNodeTypeError",
            Self::Abort => "AbortError",
            Self::TypeMismatch => "TypeMismatchError",
            Self::QuotaExceeded => "QuotaExceededError",
            Self::InvalidModification => "InvalidModificationError",
            Self::Timeout => "TimeoutError",
        }
    }

    /// Its legacy `code` (WebIDL's table).
    pub fn code(self) -> u16 {
        legacy_code(self.name())
    }
}

/// WebIDL's error names table: each name's legacy code (`DOMException`'s
/// `code`), 0 for any other (`new DOMException("m", "Foo")`).
pub fn legacy_code(name: &str) -> u16 {
    match name {
        "IndexSizeError" => 1,
        "HierarchyRequestError" => 3,
        "WrongDocumentError" => 4,
        "InvalidCharacterError" => 5,
        "NoModificationAllowedError" => 7,
        "NotFoundError" => 8,
        "NotSupportedError" => 9,
        "InUseAttributeError" => 10,
        "InvalidStateError" => 11,
        "SyntaxError" => 12,
        "InvalidModificationError" => 13,
        "NamespaceError" => 14,
        "InvalidAccessError" => 15,
        "TypeMismatchError" => 17,
        "SecurityError" => 18,
        "NetworkError" => 19,
        "AbortError" => 20,
        "URLMismatchError" => 21,
        "QuotaExceededError" => 22,
        "TimeoutError" => 23,
        "InvalidNodeTypeError" => 24,
        "DataCloneError" => 25,
        _ => 0,
    }
}

/// `DOMException`'s constants: the legacy codes by their names.
pub const LEGACY_CODES: &[(&str, u16)] = &[
    ("INDEX_SIZE_ERR", 1),
    ("DOMSTRING_SIZE_ERR", 2),
    ("HIERARCHY_REQUEST_ERR", 3),
    ("WRONG_DOCUMENT_ERR", 4),
    ("INVALID_CHARACTER_ERR", 5),
    ("NO_DATA_ALLOWED_ERR", 6),
    ("NO_MODIFICATION_ALLOWED_ERR", 7),
    ("NOT_FOUND_ERR", 8),
    ("NOT_SUPPORTED_ERR", 9),
    ("INUSE_ATTRIBUTE_ERR", 10),
    ("INVALID_STATE_ERR", 11),
    ("SYNTAX_ERR", 12),
    ("INVALID_MODIFICATION_ERR", 13),
    ("NAMESPACE_ERR", 14),
    ("INVALID_ACCESS_ERR", 15),
    ("VALIDATION_ERR", 16),
    ("TYPE_MISMATCH_ERR", 17),
    ("SECURITY_ERR", 18),
    ("NETWORK_ERR", 19),
    ("ABORT_ERR", 20),
    ("URL_MISMATCH_ERR", 21),
    ("QUOTA_EXCEEDED_ERR", 22),
    ("TIMEOUT_ERR", 23),
    ("INVALID_NODE_TYPE_ERR", 24),
    ("DATA_CLONE_ERR", 25),
];
