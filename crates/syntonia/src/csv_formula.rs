//! Neutralising and restoring spreadsheet formula markers in CSV cells.
//!
//! A CSV cell whose first character is one of a small set is evaluated as a
//! formula when the file is opened in common spreadsheet software, rather than
//! displayed. Channel names come from the operator or from a radio image, so an
//! exported name reaches a spreadsheet as attacker-influenced input.
//!
//! CSV quoting does not help: the quotes delimit the field and are stripped
//! before the cell is interpreted.
//!
//! [`neutralize`] and [`restore`] are exact inverses and live together for that
//! reason — split across the export and import modules they would be two copies
//! of one rule, free to drift into a lossy round trip.

use std::borrow::Cow;

/// Characters that begin a formula in common spreadsheet software.
///
/// `=`, `+`, `-` and `@` start an expression; a leading tab or carriage return
/// is stripped by the reader and can expose the character behind it.
const FORMULA_LEADERS: [char; 6] = ['=', '+', '-', '@', '\t', '\r'];

/// The character used to force a cell to be read as text.
const GUARD: char = '\'';

/// Returns `true` when a leading `first` needs escaping to survive a round trip.
///
/// WHY [`GUARD`] itself counts: without escaping it, `'=x` would be ambiguous —
/// either a name beginning with an apostrophe, or the guarded form of `=x` — and
/// [`restore`] could not tell them apart. Escaping the guard removes the
/// ambiguity instead of documenting it.
fn needs_guard(first: char) -> bool {
    first == GUARD || FORMULA_LEADERS.contains(&first)
}

/// Prefix `value` with [`GUARD`] if it would otherwise be read as a formula, or
/// if it already begins with the guard.
///
/// Returns the input unchanged when it is already safe, so the common case
/// allocates nothing.
pub(crate) fn neutralize(value: &str) -> Cow<'_, str> {
    match value.chars().next() {
        Some(first) if needs_guard(first) => Cow::Owned(format!("{GUARD}{value}")),
        _ => Cow::Borrowed(value),
    }
}

/// Undo [`neutralize`].
///
/// Strips one leading [`GUARD`] only when the character behind it is one this
/// module would itself have escaped. A name whose apostrophe was not added here
/// — anything CHIRP wrote, for instance — is left alone.
pub(crate) fn restore(value: &str) -> &str {
    match value.strip_prefix(GUARD) {
        Some(rest) if rest.starts_with(needs_guard) => rest,
        _ => value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every value the round-trip property is checked against, formula-leading
    /// and ordinary alike.
    const CASES: [&str; 13] = [
        "=1+1",
        "+1",
        "-1",
        "@SUM(A1)",
        "\tinjected",
        "\rinjected",
        "'=ambiguous",
        "'quoted",
        "''double",
        "CALL",
        "W1AW",
        "a=b",
        "",
    ];

    #[test]
    fn a_formula_leader_is_guarded() {
        for value in ["=1+1", "+1", "-1", "@SUM(A1)", "\tinjected", "\rinjected"] {
            let neutralized = neutralize(value);
            assert!(
                neutralized.starts_with(GUARD),
                "{value:?} must be guarded, got {neutralized:?}"
            );
        }
    }

    /// Anti-vacuity: a guard applied to everything would satisfy the case above
    /// while corrupting every ordinary name.
    #[test]
    fn an_ordinary_name_is_untouched() {
        for value in ["CALL", "146.520", "W1AW", "", "a=b"] {
            assert_eq!(
                neutralize(value),
                Cow::Borrowed(value),
                "{value:?} is not a formula and must pass through unchanged"
            );
        }
    }

    #[test]
    fn neutralize_and_restore_are_inverses() {
        for value in CASES {
            assert_eq!(
                restore(&neutralize(value)),
                value,
                "the round trip must return {value:?} exactly"
            );
        }
    }

    /// The half that keeps the inverse honest: `restore` must not eat an
    /// apostrophe this module did not add, which is what a file written by
    /// other software would contain.
    #[test]
    fn restore_leaves_a_foreign_apostrophe_alone() {
        assert_eq!(restore("'CALL"), "'CALL");
        assert_eq!(restore("'"), "'");
    }

    /// The reason [`needs_guard`] includes the guard itself.
    #[test]
    fn a_guarded_formula_is_distinguishable_from_a_quoted_one() {
        assert_eq!(neutralize("=x"), "'=x");
        assert_eq!(neutralize("'=x"), "''=x");
        assert_ne!(
            neutralize("=x"),
            neutralize("'=x"),
            "two different names must not encode to the same cell"
        );
    }
}
