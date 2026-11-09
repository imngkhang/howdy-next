//! Model label rules, shared by the codec and the CLI.

use std::fmt::Write as _;

use crate::MAX_LABEL_BYTES;

/// Why a label is not valid for a new entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelProblem {
    /// The label is empty.
    Empty,
    /// The label is longer than [`MAX_LABEL_BYTES`] and cutting it to fit would make it valid.
    TooLong,
    /// The label is not UTF-8, or holds `/`, `\`, or a control character.
    Malformed,
    /// The label holds an invisible format character (see [`is_hidden_format`]).
    Hidden,
}

/// What, if anything, makes `label` invalid for a new entry. New labels must be stored labels
/// (see [`stored_label`]) that also display as stored. Only writers of new entries (append and
/// the CLI) apply this; the codec reads and re-writes existing labels with [`stored_label`], so a
/// tighter display rule never blocks authentication or later edits.
#[must_use]
pub fn label_problem(label: &[u8]) -> Option<LabelProblem> {
    if label.is_empty() {
        return Some(LabelProblem::Empty);
    }
    if label.len() > MAX_LABEL_BYTES {
        // Report the length only when cutting to fit fixes the label; otherwise report what
        // cutting would still leave wrong.
        return match label_problem(&label[..label_prefix_len(label)]) {
            None | Some(LabelProblem::Empty) => Some(LabelProblem::TooLong),
            problem => problem,
        };
    }
    match stored_label(label) {
        None => Some(LabelProblem::Malformed),
        Some(text) if text.chars().any(is_hidden_format) => Some(LabelProblem::Hidden),
        Some(_) => None,
    }
}

/// Whether `label` is valid for a new entry (see [`label_problem`]).
#[must_use]
pub fn is_valid_label(label: &[u8]) -> bool {
    label_problem(label).is_none()
}

/// Longest text [`display_label`] returns: every byte of a label escaped as `<U+XXXX>`.
pub const MAX_DISPLAY_LABEL_BYTES: usize = MAX_LABEL_BYTES * 8;

/// `label` for a terminal: invisible format and control characters become `<U+XXXX>`, so a
/// stored label that predates or bypassed the display rule cannot disguise itself.
#[must_use]
pub fn display_label(label: &str) -> String {
    let mut shown = String::with_capacity(label.len());
    for c in label.chars() {
        if c.is_control() || is_hidden_format(c) {
            // Writing to a String cannot fail.
            let _ = write!(shown, "<U+{:04X}>", u32::from(c));
        } else {
            shown.push(c);
        }
    }
    shown
}

/// `label` as text when it is safe to read back: 1 to [`MAX_LABEL_BYTES`] bytes of UTF-8 with no
/// `/`, `\`, or control character (C0, DEL, or C1). Decoding applies only this rule, so tighter
/// display rules never make an existing file unreadable during authentication.
pub(crate) fn stored_label(label: &[u8]) -> Option<&str> {
    if label.is_empty() || label.len() > MAX_LABEL_BYTES {
        return None;
    }
    let text = std::str::from_utf8(label).ok()?;
    (!text
        .chars()
        .any(|c| c == '/' || c == '\\' || c.is_control()))
    .then_some(text)
}

/// Bidi controls, zero-width spaces, and separators that let a label display differently from
/// what is stored, for example in the `howdy remove` confirmation. ZWJ and ZWNJ (U+200C/U+200D)
/// stay allowed: Persian, Indic scripts, and emoji sequences need them.
fn is_hidden_format(c: char) -> bool {
    matches!(
        c,
        '\u{061C}'                       // Arabic letter mark
            | '\u{180E}'                 // Mongolian vowel separator
            | '\u{200B}'                 // zero-width space
            | '\u{200E}'..='\u{200F}'     // LRM, RLM
            | '\u{2028}'..='\u{202E}'     // line/paragraph separators, bidi embeddings/overrides
            | '\u{2060}'..='\u{2064}'     // word joiner, invisible operators
            | '\u{2066}'..='\u{2069}'     // bidi isolates
            | '\u{FEFF}' // zero-width no-break space
    )
}

/// Length of the longest prefix of `text` that is at most [`MAX_LABEL_BYTES`] bytes and does not
/// split a UTF-8 sequence. It only cuts; the result still has to pass [`is_valid_label`].
#[must_use]
pub fn label_prefix_len(text: &[u8]) -> usize {
    if text.len() <= MAX_LABEL_BYTES {
        return text.len();
    }
    let mut cut = MAX_LABEL_BYTES;
    while cut > 0 && text[cut] & 0xC0 == 0x80 {
        cut -= 1;
    }
    cut
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain_and_multibyte_labels() {
        assert!(is_valid_label(b"Model #0"));
        assert!(is_valid_label("Café 🙂".as_bytes()));
        assert!(is_valid_label(&[b'x'; MAX_LABEL_BYTES]));
    }

    #[test]
    fn rejects_bad_labels() {
        assert!(!is_valid_label(b""));
        assert!(!is_valid_label(&[b'x'; MAX_LABEL_BYTES + 1]));
        assert!(!is_valid_label(b"a/b"));
        assert!(!is_valid_label(b"a\\b"));
        assert!(!is_valid_label(b"a\nb"));
        assert!(!is_valid_label(b"a\x7fb"));
        assert!(!is_valid_label("a\u{9b}b".as_bytes()), "C1 control");
        assert!(!is_valid_label(b"\xC0\x80"), "overlong");
        assert!(!is_valid_label(b"\xED\xA0\x80"), "surrogate");
        assert!(!is_valid_label(b"\xF4\x90\x80\x80"), "above U+10FFFF");
        assert!(!is_valid_label(b"\xE2\x82"), "truncated sequence");
        for hidden in [
            '\u{061C}', '\u{180E}', '\u{200B}', '\u{200F}', '\u{2028}', '\u{202E}', '\u{2060}',
            '\u{2066}', '\u{2069}', '\u{FEFF}',
        ] {
            let label = format!("ab{hidden}cd");
            assert!(
                !is_valid_label(label.as_bytes()),
                "hidden format U+{:04X}",
                u32::from(hidden)
            );
        }
        assert!(
            is_valid_label("ก้อง".as_bytes()),
            "combining marks stay valid"
        );
        assert!(
            is_valid_label("می\u{200C}خواهم".as_bytes()),
            "Persian ZWNJ stays valid"
        );
        assert!(
            is_valid_label("👨\u{200D}👩\u{200D}👧".as_bytes()),
            "emoji ZWJ stays valid"
        );
    }

    #[test]
    fn stored_labels_tolerate_display_rules() {
        let hidden = "ab\u{202E}cd";
        assert_eq!(stored_label(hidden.as_bytes()), Some(hidden));
        assert!(!is_valid_label(hidden.as_bytes()));
        assert_eq!(stored_label(b"a/b"), None);
    }

    #[test]
    fn problems_name_the_cause() {
        assert_eq!(label_problem(b""), Some(LabelProblem::Empty));
        assert_eq!(label_problem(b"a/b"), Some(LabelProblem::Malformed));
        assert_eq!(
            label_problem("a\u{200B}b".as_bytes()),
            Some(LabelProblem::Hidden)
        );
        let long = "y".repeat(MAX_LABEL_BYTES + 1);
        assert_eq!(label_problem(long.as_bytes()), Some(LabelProblem::TooLong));
        assert_eq!(
            label_problem(b"office/desk-camera-left-side"),
            Some(LabelProblem::Malformed),
            "a long label that is also malformed reports the malformation"
        );
        assert_eq!(label_problem(b"desk"), None);
    }

    #[test]
    fn display_escapes_hidden_and_control_characters() {
        assert_eq!(display_label("desk"), "desk");
        assert_eq!(display_label("ก้อง"), "ก้อง");
        assert_eq!(display_label("ab\u{202E}cd"), "ab<U+202E>cd");
        assert_eq!(display_label("a\u{1}b"), "a<U+0001>b");
        let worst = "\u{1}".repeat(MAX_LABEL_BYTES);
        assert_eq!(display_label(&worst).len(), MAX_DISPLAY_LABEL_BYTES);
    }

    #[test]
    fn prefix_keeps_short_text() {
        assert_eq!(label_prefix_len(b"short"), 5);
        assert_eq!(label_prefix_len(&[b'x'; MAX_LABEL_BYTES]), MAX_LABEL_BYTES);
    }

    #[test]
    fn prefix_cuts_on_code_point_boundary() {
        // 23 ASCII bytes, then a 4-byte emoji straddling the limit.
        let text = format!("{}🙂tail", "x".repeat(MAX_LABEL_BYTES - 1));
        let cut = label_prefix_len(text.as_bytes());
        assert_eq!(cut, MAX_LABEL_BYTES - 1);
        assert!(is_valid_label(&text.as_bytes()[..cut]));

        let ascii = "y".repeat(MAX_LABEL_BYTES + 5);
        assert_eq!(label_prefix_len(ascii.as_bytes()), MAX_LABEL_BYTES);
    }

    #[test]
    fn prefix_of_only_continuation_bytes_is_empty() {
        assert_eq!(label_prefix_len(&[0x80; MAX_LABEL_BYTES + 1]), 0);
    }
}
