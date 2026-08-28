//! Identity types.
//!
//! Ids are the keys everything else is filed under, so they are parsed into a
//! **single canonical form** and never repaired into one. That rule is
//! expensive to learn the other way: a system that accepted two spellings of
//! one account — checksummed and lower-cased — filed every execution twice and
//! double-counted P&L that had never happened. A key that two writers can spell
//! differently is not a key.

use std::fmt;

use crate::DomainError;

/// A competition participant. Sorts as a total order, which the ranking
/// tiebreak in `docs/ranking.md` §2 relies on as its final level.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ParticipantId(String);

/// A tradable instrument. Canonical form is upper-case.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Symbol(String);

/// **Our** order id, minted the instant we decide to submit — before the broker
/// has acked, which is what makes cancel-before-ack expressible at all
/// (`docs/design.md` §4).
///
/// This is FIX `ClOrdID` (tag 11). The pairing with [`BrokerOrderId`] is not an
/// invention: FIX carries both on every execution report, and a cancel/replace
/// mints a *new* `ClOrdID` pointing at the old one via `OrigClOrdID` (tag 41) —
/// which is exactly the `replaces` link in §5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClientOrderId(u64);

/// The **broker's** order id, recorded once known. Absent until the ack lands.
///
/// FIX `OrderID` (tag 37). Two ids rather than one because there is a real
/// window between submit and ack in which the order exists and the broker's id
/// does not — and in that window it still has to be cancellable, loggable and
/// correlatable. Keying the registry on this id instead would also drop any
/// execution report that arrives before the ack is processed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BrokerOrderId(u64);

impl ParticipantId {
    pub fn parse(s: &str) -> Result<Self, DomainError> {
        parse_ident(
            "participant id",
            s,
            64,
            "ASCII letters, digits, '-' or '_'",
            |b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_',
        )
        .map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Symbol {
    /// Parses only the canonical upper-case form. `"aapl"` is **rejected, not
    /// upper-cased** — see the module note: normalising an id on the way in
    /// means two callers can disagree about the key and neither is wrong.
    pub fn parse(s: &str) -> Result<Self, DomainError> {
        parse_ident("symbol", s, 16, "upper-case ASCII, digits or '.'", |b| {
            b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'.'
        })
        .map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl ClientOrderId {
    pub const fn new(n: u64) -> Self {
        Self(n)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl BrokerOrderId {
    pub const fn new(n: u64) -> Self {
        Self(n)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

fn parse_ident(
    field: &'static str,
    s: &str,
    max: usize,
    allowed: &'static str,
    ok: impl Fn(u8) -> bool,
) -> Result<String, DomainError> {
    if s.is_empty() || s.len() > max || !s.bytes().all(ok) {
        return Err(DomainError::ParseIdent {
            field,
            max,
            allowed,
            input: s.to_owned(),
        });
    }
    Ok(s.to_owned())
}

impl fmt::Display for ParticipantId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Display for ClientOrderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Display for BrokerOrderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_canonical_identifiers() {
        assert_eq!(
            ParticipantId::parse("alice_01").unwrap().as_str(),
            "alice_01"
        );
        assert_eq!(Symbol::parse("AAPL").unwrap().as_str(), "AAPL");
        assert_eq!(Symbol::parse("BRK.B").unwrap().as_str(), "BRK.B");
    }

    #[test]
    fn rejects_non_canonical_symbols_rather_than_normalising() {
        // The whole point: "aapl" and "AAPL" must not both become one key by a
        // path the caller cannot see. One spelling, or an error.
        assert!(Symbol::parse("aapl").is_err());
        assert!(Symbol::parse("Aapl").is_err());
    }

    #[test]
    fn rejects_empty_oversized_and_illegal_characters() {
        assert!(ParticipantId::parse("").is_err());
        assert!(ParticipantId::parse(&"a".repeat(65)).is_err());
        assert!(ParticipantId::parse("alice smith").is_err());
        assert!(ParticipantId::parse("alice@example.com").is_err());
        assert!(Symbol::parse("").is_err());
    }

    #[test]
    fn participant_ids_form_a_total_order() {
        // The final tiebreak in docs/ranking.md depends on this: no two
        // distinct participants may ever compare equal.
        let mut ids: Vec<_> = ["carol", "alice", "bob"]
            .iter()
            .map(|s| ParticipantId::parse(s).unwrap())
            .collect();
        ids.sort();
        let sorted: Vec<_> = ids.iter().map(|i| i.as_str()).collect();
        assert_eq!(sorted, ["alice", "bob", "carol"]);
    }
}
