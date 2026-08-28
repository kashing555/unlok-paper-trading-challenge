use thiserror::Error;

/// Every way a domain value can refuse to be constructed or combined.
///
/// Typed rather than stringly (`.claude/rust.md`) so a caller can distinguish a
/// rejected input from a broken invariant: the API layer maps `Parse*` and
/// `NonPositive*` to a 400 and `Overflow` to a 500, which it could not do
/// against a message.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DomainError {
    /// The `Qty` type invariant: long-only, so shares are never negative.
    #[error("quantity must not be negative, got {0}")]
    NegativeQty(i64),

    /// The *order* rule, distinct from the type invariant above: zero is a
    /// legal `Qty` (an unfilled order has `filled == 0`, a closed position has
    /// `qty == 0`) but an order *for* zero shares is meaningless. Enforced at
    /// the order constructor in A1, not here.
    #[error("order quantity must be positive, got {0}")]
    NonPositiveQty(i64),

    #[error("price must be positive, got {0}")]
    NonPositivePx(i64),

    #[error("arithmetic overflow in {0}")]
    Overflow(&'static str),

    #[error("{field}: expected a decimal with at most {max} places, got {input:?}")]
    TooManyDecimals {
        field: &'static str,
        max: usize,
        input: String,
    },

    #[error("{field}: not a valid decimal: {input:?}")]
    ParseDecimal { field: &'static str, input: String },

    #[error("{field}: must be negative-free, got {input:?}")]
    UnexpectedSign { field: &'static str, input: String },

    #[error("{field}: must be 1..={max} characters of {allowed}, got {input:?}")]
    ParseIdent {
        field: &'static str,
        max: usize,
        allowed: &'static str,
        input: String,
    },

    #[error("not a valid ISO-8601 date (YYYY-MM-DD): {0:?}")]
    ParseDate(String),
}
