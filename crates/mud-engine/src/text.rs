//! Player-input safety for authored text (§3.6.4, §3.20.7).
//!
//! Player-authored message bodies (`say`, `emote`, and downstream `tell` /
//! channel sends) pass through [`sanitize`] before the engine builds output
//! spans from them. It enforces the §3.6.4 4 KiB content cap (rejecting, never
//! truncating), strips control characters other than `\n`, and removes raw ANSI
//! escape sequences so a player cannot smuggle terminal control codes into
//! another player's stream.
//!
//! Markup safety (§3.20.7.1) is handled at the emission site, not here: a
//! sanitized body becomes a plain [`Span`](mud_core::Span), never compiled
//! through the builder markup path, so `{role}` braces in player text render
//! literally and cannot inject styling.

use unicode_normalization::UnicodeNormalization;

/// The §3.6.4 content cap: 4 KiB of UTF-8 after normalization.
pub const MAX_CONTENT_BYTES: usize = 4096;

/// A player message body exceeding the §3.6.4 content cap.
///
/// Surfaced to the player as a command message, not a pipeline error: an
/// over-long line is a normal player mistake, not a fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("message exceeds the 4 KiB content cap")]
#[non_exhaustive]
pub struct ContentTooLong;

/// Normalizes and sanitizes a player-authored message body (§3.6.4).
///
/// Returns the cleaned text, or [`ContentTooLong`] if it exceeds
/// [`MAX_CONTENT_BYTES`] after Unicode normalization. The result has control
/// characters other than `\n` and raw ANSI escape sequences removed; it is never
/// silently truncated.
pub fn sanitize(raw: &str) -> Result<String, ContentTooLong> {
    let normalized: String = raw.nfc().collect();
    if normalized.len() > MAX_CONTENT_BYTES {
        return Err(ContentTooLong);
    }
    Ok(strip(&normalized))
}

/// Removes ANSI escape sequences and control characters other than `\n`.
///
/// The two escape forms a player might use to inject styling or terminal
/// commands are consumed whole: CSI (`ESC [ … final`, the SGR colour vector
/// among them) and OSC (`ESC ] … BEL`/`ST`, e.g. a window-title set). A
/// malformed sequence is abandoned at the first byte that cannot belong to it —
/// crucially a `\n` is never swallowed by an unterminated sequence. Any other
/// `ESC` (or stray control byte) is dropped on its own, so no terminal control
/// byte survives into output.
fn strip(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.peek() {
                Some('[') => {
                    let _ = chars.next();
                    consume_csi(&mut chars);
                }
                Some(']') => {
                    let _ = chars.next();
                    consume_osc(&mut chars);
                }
                // A lone or otherwise-shaped escape: drop just the ESC; the rest
                // is left to the control-char filter below (its printable bytes
                // survive as literal text — harmless, never a control byte).
                _ => {}
            }
            continue;
        }
        if c == '\n' || !c.is_control() {
            out.push(c);
        }
    }
    out
}

/// Consumes a CSI sequence body after `ESC [`: parameter/intermediate bytes
/// (`0x20..=0x3f`) followed by one final byte (`0x40..=0x7e`). Stops at the
/// first byte that fits neither — without consuming it — so a malformed
/// sequence cannot eat trailing text (e.g. a newline).
fn consume_csi(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while let Some(&p) = chars.peek() {
        if ('\u{20}'..='\u{3f}').contains(&p) {
            let _ = chars.next();
        } else if ('\u{40}'..='\u{7e}').contains(&p) {
            let _ = chars.next();
            break;
        } else {
            break;
        }
    }
}

/// Consumes an OSC sequence body after `ESC ]`, up to and including its `BEL`
/// or `ST` (`ESC \`) terminator. Stops without consuming a `\n`, so an
/// unterminated sequence cannot eat following lines.
fn consume_osc(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while let Some(&p) = chars.peek() {
        match p {
            '\u{07}' => {
                let _ = chars.next();
                break;
            }
            '\u{1b}' => {
                let _ = chars.next();
                if chars.peek() == Some(&'\\') {
                    let _ = chars.next();
                }
                break;
            }
            '\n' => break,
            _ => {
                let _ = chars.next();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_text_passes_through_unchanged() {
        assert_eq!(sanitize("Hello, world!").as_deref(), Ok("Hello, world!"));
    }

    #[test]
    fn an_over_cap_message_is_rejected_not_truncated() {
        let long = "x".repeat(MAX_CONTENT_BYTES + 1);

        assert_eq!(sanitize(&long), Err(ContentTooLong));
    }

    #[test]
    fn a_message_exactly_at_the_cap_is_accepted() {
        let at_cap = "x".repeat(MAX_CONTENT_BYTES);

        assert_eq!(sanitize(&at_cap).map(|s| s.len()), Ok(MAX_CONTENT_BYTES));
    }

    #[test]
    fn raw_ansi_colour_codes_are_stripped() {
        // A player trying to colour their speech red: the SGR sequence is gone,
        // only the words survive.
        let injected = "\u{1b}[31mred\u{1b}[0m text";

        assert_eq!(sanitize(injected).as_deref(), Ok("red text"));
    }

    #[test]
    fn control_chars_other_than_newline_are_stripped() {
        let noisy = "a\tb\r\nc\u{7}d";

        assert_eq!(sanitize(noisy).as_deref(), Ok("ab\ncd"));
    }

    #[test]
    fn a_lone_escape_leaves_no_control_byte() {
        let stray = "before\u{1b}after";

        assert_eq!(sanitize(stray).as_deref(), Ok("beforeafter"));
    }

    #[test]
    fn an_osc_sequence_is_consumed_whole() {
        // A window-title set: `ESC ] 0 ; pwned BEL`. The payload must not leak as
        // visible text the way a bare ESC + literal body would.
        let osc = "\u{1b}]0;pwned\u{7}hi";

        assert_eq!(sanitize(osc).as_deref(), Ok("hi"));
    }

    #[test]
    fn a_malformed_csi_does_not_swallow_a_following_newline() {
        // `ESC [ 1` with no final byte, then a newline the player meant to keep.
        let malformed = "a\u{1b}[1\nb";

        assert_eq!(sanitize(malformed).as_deref(), Ok("a\nb"));
    }
}
