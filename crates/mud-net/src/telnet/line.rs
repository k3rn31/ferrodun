//! Line decoding for the telnet data stream.
//!
//! Accumulates data bytes (post-IAC-parsing) into lines. Telnet line endings
//! are CR LF and CR NUL (RFC 854); bare LF is tolerated because common MUD
//! clients send it. Oversized lines are dropped whole — a truncated command
//! must never execute (design decision, see the M1-20 design doc).

/// Maximum accepted line length in bytes; longer lines are dropped whole.
pub(crate) const MAX_LINE_BYTES: usize = 4096;

/// Incremental line decoder; one per connection.
#[derive(Debug)]
pub(crate) struct LineDecoder {
    buf: Vec<u8>,
    overflowed: bool,
    swallow_lf_or_nul: bool,
}

impl LineDecoder {
    pub(crate) fn new() -> Self {
        Self {
            buf: Vec::new(),
            overflowed: false,
            swallow_lf_or_nul: false,
        }
    }

    /// Feeds one data byte; returns a completed line when a terminator arrives.
    ///
    /// Invalid UTF-8 is replaced with U+FFFD. A line exceeding
    /// [`MAX_LINE_BYTES`] yields `None` at its terminator and is discarded.
    pub(crate) fn push(&mut self, byte: u8) -> Option<String> {
        if std::mem::take(&mut self.swallow_lf_or_nul) && (byte == b'\n' || byte == 0) {
            return None;
        }
        match byte {
            b'\r' => {
                self.swallow_lf_or_nul = true;
                self.finish()
            }
            b'\n' => self.finish(),
            data => {
                if self.buf.len() >= MAX_LINE_BYTES {
                    self.overflowed = true;
                } else {
                    self.buf.push(data);
                }
                None
            }
        }
    }

    fn finish(&mut self) -> Option<String> {
        let overflowed = std::mem::take(&mut self.overflowed);
        let bytes = std::mem::take(&mut self.buf);
        if overflowed {
            None
        } else {
            Some(String::from_utf8_lossy(&bytes).into_owned())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_all(decoder: &mut LineDecoder, bytes: &[u8]) -> Vec<String> {
        bytes.iter().filter_map(|&b| decoder.push(b)).collect()
    }

    #[test]
    fn crlf_terminates_a_line() {
        let mut decoder = LineDecoder::new();
        assert_eq!(push_all(&mut decoder, b"look\r\n"), vec!["look".to_owned()]);
    }

    #[test]
    fn cr_nul_terminates_a_line() {
        let mut decoder = LineDecoder::new();
        assert_eq!(
            push_all(&mut decoder, b"north\r\0"),
            vec!["north".to_owned()]
        );
    }

    #[test]
    fn bare_lf_is_tolerated_as_terminator() {
        let mut decoder = LineDecoder::new();
        assert_eq!(push_all(&mut decoder, b"south\n"), vec!["south".to_owned()]);
    }

    #[test]
    fn two_lines_split_across_pushes() {
        let mut decoder = LineDecoder::new();
        let mut lines = push_all(&mut decoder, b"say hel");
        lines.extend(push_all(&mut decoder, b"lo\r\nwho\r\n"));
        assert_eq!(lines, vec!["say hello".to_owned(), "who".to_owned()]);
    }

    #[test]
    fn empty_line_is_emitted() {
        let mut decoder = LineDecoder::new();
        assert_eq!(push_all(&mut decoder, b"\r\n"), vec![String::new()]);
    }

    #[test]
    fn invalid_utf8_is_replaced_lossily() {
        let mut decoder = LineDecoder::new();
        let lines = push_all(&mut decoder, &[b'h', 0xC3, b'\r', b'\n']);
        assert_eq!(lines, vec!["h\u{FFFD}".to_owned()]);
    }

    #[test]
    fn oversized_line_is_dropped_without_event_and_decoder_recovers() {
        let mut decoder = LineDecoder::new();
        let mut input = vec![b'x'; MAX_LINE_BYTES + 1];
        input.extend(b"\r\nok\r\n");
        // The oversized line yields no event; the next line decodes normally.
        assert_eq!(push_all(&mut decoder, &input), vec!["ok".to_owned()]);
    }

    #[test]
    fn line_exactly_at_cap_is_emitted() {
        let mut decoder = LineDecoder::new();
        let mut input = vec![b'x'; MAX_LINE_BYTES];
        input.extend(b"\r\n");
        let lines = push_all(&mut decoder, &input);
        assert_eq!(lines.len(), 1, "a line exactly at the cap is not oversized");
        assert_eq!(
            lines.first().map(String::len),
            Some(MAX_LINE_BYTES),
            "full content is preserved"
        );
    }
}
