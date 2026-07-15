//! Telnet protocol core (SPEC §2.8.2, M1 subset).
//!
//! Sans-IO: bytes in, typed events plus reply bytes out. The gateway (M1-21)
//! owns the socket and drives this state machine.

mod line;
mod negotiation;
mod parser;

use line::LineDecoder;
use negotiation::{CharsetMode, Negotiator, OPT_CHARSET, OPT_NAWS, OPT_TTYPE, TTYPE_IS};
use parser::{EOR_CMD, GA, IAC, IacParser, ParsedItem};

/// A validated, decoded event from the client.
///
/// A transport DTO: fields stay raw (`String`, `u16`) because parsing them into
/// domain values (`InputLine`, tier from terminal type, render width) belongs to
/// the gateway above, which owns those types. `mud-net` sits below the domain
/// crates, so wrapping here would invert the dependency or add empty newtypes.
#[non_exhaustive]
#[derive(Debug, PartialEq, Eq)]
pub enum TelnetEvent {
    /// One complete input line (a command), lossily decoded to UTF-8.
    Line(String),
    /// Client window size from NAWS; 0 means "unspecified" per RFC 1073.
    WindowSize { width: u16, height: u16 },
    /// Client terminal name from TTYPE.
    TerminalType(String),
}

/// Client-side local echo, controlled by the server via RFC 857.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalEcho {
    /// Normal input: the client echoes what the player types.
    Enabled,
    /// Secret entry: the client is asked to stop echoing (IAC WILL ECHO).
    Suppressed,
}

/// Per-connection telnet protocol state machine (sans-IO).
///
/// Feed raw socket bytes to [`receive`](Self::receive) and get decoded
/// [`TelnetEvent`]s; negotiation replies accumulate internally and must be
/// drained with [`take_output`](Self::take_output) and written to the client.
/// Construction queues the M1 opening offers (NAWS, TTYPE, EOR, CHARSET).
#[derive(Debug)]
pub struct TelnetMachine {
    parser: IacParser,
    negotiator: Negotiator,
    line: LineDecoder,
    output: Vec<u8>,
}

impl TelnetMachine {
    /// Creates the machine with the opening option offers already queued.
    #[must_use]
    pub fn new() -> Self {
        let mut output = Vec::new();
        let negotiator = Negotiator::new(&mut output);
        Self {
            parser: IacParser::new(),
            negotiator,
            line: LineDecoder::new(),
            output,
        }
    }

    /// Consumes raw bytes from the socket, returning completed events.
    ///
    /// Negotiation replies triggered by the input are queued; drain them with
    /// [`take_output`](Self::take_output) after each call.
    pub fn receive(&mut self, bytes: &[u8]) -> Vec<TelnetEvent> {
        let mut events = Vec::new();
        for item in self.parser.push(bytes) {
            match item {
                ParsedItem::Data(byte) => {
                    if let Some(text) = self.line.push(byte) {
                        // A client that agreed to suppress echo shows nothing
                        // — not even the Enter — so advance its display past
                        // the prompt line (design 2026-07-11 §4).
                        if self.negotiator.echo_suppressed() {
                            self.output.extend_from_slice(b"\r\n");
                        }
                        events.push(TelnetEvent::Line(text));
                    }
                }
                ParsedItem::Command(_) => {}
                ParsedItem::Negotiate { verb, option } => {
                    self.negotiator.on_negotiate(verb, option, &mut self.output);
                }
                ParsedItem::Subnegotiation { option, payload } => {
                    self.on_subnegotiation(option, &payload, &mut events);
                }
            }
        }
        events
    }

    /// Asks the client to change its local echo (RFC 857, password masking).
    /// The negotiation bytes accumulate internally; drain them with
    /// [`take_output`](Self::take_output) and write them to the client
    /// before the prompt they guard.
    pub fn set_echo(&mut self, echo: LocalEcho) {
        match echo {
            LocalEcho::Suppressed => self.negotiator.suppress_echo(&mut self.output),
            LocalEcho::Enabled => self.negotiator.restore_echo(&mut self.output),
        }
    }

    /// Drains the bytes the server must write to the client.
    #[must_use]
    pub fn take_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.output)
    }

    /// Encodes server→client text per the negotiated charset: UTF-8
    /// passthrough, or ASCII transliteration for legacy clients. Normalizes
    /// LF to CRLF and escapes any literal IAC byte.
    #[must_use]
    pub fn encode_output(&self, text: &str) -> Vec<u8> {
        let encoded: std::borrow::Cow<'_, str> = match self.negotiator.charset() {
            CharsetMode::Utf8 => std::borrow::Cow::Borrowed(text),
            // deunicode maps control bytes to "" once it hits its
            // transliteration path, so ANSI CSI escapes (a later task renders
            // color before this point) must be shielded from it.
            CharsetMode::Ascii => std::borrow::Cow::Owned(transliterate_preserving_escapes(text)),
        };
        let mut out = Vec::with_capacity(encoded.len() + 8);
        for &byte in encoded.as_bytes() {
            match byte {
                b'\n' => out.extend_from_slice(b"\r\n"),
                // Dropped: CRLF in input is re-emitted via the '\n' arm, so
                // carriage returns never double.
                b'\r' => {}
                IAC => out.extend_from_slice(&[IAC, IAC]),
                other => out.push(other),
            }
        }
        out
    }

    /// The prompt-framing byte pair: IAC EOR when negotiated, else IAC GA.
    #[must_use]
    pub fn prompt_frame(&self) -> Vec<u8> {
        if self.negotiator.eor_enabled() {
            vec![IAC, EOR_CMD]
        } else {
            vec![IAC, GA]
        }
    }

    fn on_subnegotiation(&mut self, option: u8, payload: &[u8], events: &mut Vec<TelnetEvent>) {
        match option {
            OPT_NAWS => {
                // RFC 1073: exactly four bytes, width and height big-endian.
                if let &[w_hi, w_lo, h_hi, h_lo] = payload {
                    events.push(TelnetEvent::WindowSize {
                        width: u16::from_be_bytes([w_hi, w_lo]),
                        height: u16::from_be_bytes([h_hi, h_lo]),
                    });
                }
            }
            OPT_TTYPE => {
                if let Some((&TTYPE_IS, name)) = payload.split_first() {
                    events.push(TelnetEvent::TerminalType(
                        String::from_utf8_lossy(name).into_owned(),
                    ));
                }
            }
            OPT_CHARSET => self.negotiator.on_charset_subnegotiation(payload),
            _ => {}
        }
    }
}

impl Default for TelnetMachine {
    fn default() -> Self {
        Self::new()
    }
}

/// Transliterates text to ASCII for legacy clients while passing ANSI CSI
/// escape sequences (SGR color codes, etc.) through untouched — deunicode
/// would otherwise eat the ESC byte and leak the raw escape text, since a
/// later task renders ANSI color at the gateway before this legacy-charset
/// path runs.
fn transliterate_preserving_escapes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('\u{1b}') {
        // INVARIANT: '\u{1b}' (ESC) is a one-byte ASCII char, so `start` and
        // `start + escape_len(...)` both land on UTF-8 char boundaries and
        // `split_at` cannot panic.
        let (plain, from_esc) = rest.split_at(start);
        out.push_str(&transliterate_plain(plain));
        let (escape, remainder) = from_esc.split_at(escape_len(from_esc));
        out.push_str(escape);
        rest = remainder;
    }
    out.push_str(&transliterate_plain(rest));
    out
}

/// Length in bytes of the escape sequence starting at `s` (which begins with
/// ESC): a CSI sequence runs from ESC through its final byte
/// (`0x40..=0x7e`); a lone ESC not opening a CSI sequence passes through one
/// byte.
fn escape_len(s: &str) -> usize {
    let bytes = s.as_bytes();
    if bytes.get(1) != Some(&b'[') {
        return 1;
    }
    bytes
        .iter()
        .skip(2)
        .position(|byte| (0x40..=0x7e).contains(byte))
        .map_or(bytes.len(), |i| i + 3)
}

/// deunicode maps control bytes to "" once it hits its transliteration path,
/// so '\n' must be shielded from it by transliterating line-by-line rather
/// than the whole string.
fn transliterate_plain(text: &str) -> String {
    text.split('\n')
        .map(deunicode::deunicode)
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::negotiation::{OPT_NAWS, OPT_TTYPE, TTYPE_IS};
    use super::parser::{DO, IAC, SB, SE, WILL};
    use super::*;

    #[test]
    fn new_machine_queues_opening_offers() {
        let mut machine = TelnetMachine::new();
        let out = machine.take_output();
        assert!(
            !out.is_empty(),
            "opening offers must be queued at construction"
        );
        assert_eq!(out.first(), Some(&IAC));
    }

    #[test]
    fn take_output_drains() {
        let mut machine = TelnetMachine::new();
        let _ = machine.take_output();
        assert!(
            machine.take_output().is_empty(),
            "second take_output must be empty"
        );
    }

    #[test]
    fn command_line_becomes_line_event() {
        let mut machine = TelnetMachine::new();
        let events = machine.receive(b"look\r\n");
        assert_eq!(events, vec![TelnetEvent::Line("look".to_owned())]);
    }

    #[test]
    fn naws_subnegotiation_becomes_window_size_event() {
        let mut machine = TelnetMachine::new();
        let _ = machine.receive(&[IAC, WILL, OPT_NAWS]);
        let events = machine.receive(&[IAC, SB, OPT_NAWS, 0, 80, 0, 24, IAC, SE]);
        assert_eq!(
            events,
            vec![TelnetEvent::WindowSize {
                width: 80,
                height: 24
            }]
        );
    }

    #[test]
    fn malformed_naws_payload_is_ignored() {
        let mut machine = TelnetMachine::new();
        let events = machine.receive(&[IAC, SB, OPT_NAWS, 0, 80, IAC, SE]);
        assert!(
            events.is_empty(),
            "a 2-byte NAWS payload must not produce an event"
        );
    }

    #[test]
    fn ttype_is_becomes_terminal_type_event() {
        let mut machine = TelnetMachine::new();
        let mut sub = vec![IAC, SB, OPT_TTYPE, TTYPE_IS];
        sub.extend_from_slice(b"MUDLET");
        sub.extend_from_slice(&[IAC, SE]);
        let events = machine.receive(&sub);
        assert_eq!(events, vec![TelnetEvent::TerminalType("MUDLET".to_owned())]);
    }

    #[test]
    fn negotiation_replies_are_queued_to_output() {
        let mut machine = TelnetMachine::new();
        let _ = machine.take_output(); // discard opening offers
        let events = machine.receive(&[IAC, WILL, OPT_TTYPE]);
        assert!(events.is_empty(), "negotiation produces output, not events");
        assert_eq!(machine.take_output(), vec![IAC, SB, OPT_TTYPE, 1, IAC, SE]);
    }

    #[test]
    fn data_interleaved_with_negotiation_decodes() {
        let mut machine = TelnetMachine::new();
        let mut input = b"lo".to_vec();
        input.extend_from_slice(&[IAC, WILL, OPT_NAWS]);
        input.extend_from_slice(b"ok\r\n");
        let events = machine.receive(&input);
        assert_eq!(events, vec![TelnetEvent::Line("look".to_owned())]);
    }

    #[test]
    fn do_unsupported_is_refused() {
        let mut machine = TelnetMachine::new();
        let _ = machine.take_output();
        let _ = machine.receive(&[IAC, DO, 86]); // MCCP2: M3, refused in M1
        assert_eq!(machine.take_output(), vec![IAC, 252, 86]); // IAC WONT 86
    }

    use super::negotiation::{CHARSET_ACCEPTED, OPT_CHARSET, OPT_EOR};
    use super::parser::{DONT, EOR_CMD, GA};

    fn utf8_machine() -> TelnetMachine {
        let mut machine = TelnetMachine::new();
        let _ = machine.receive(&[IAC, DO, OPT_CHARSET]);
        let mut accepted = vec![IAC, SB, OPT_CHARSET, CHARSET_ACCEPTED];
        accepted.extend_from_slice(b"UTF-8");
        accepted.extend_from_slice(&[IAC, SE]);
        let _ = machine.receive(&accepted);
        machine
    }

    #[test]
    fn utf8_client_gets_utf8_passthrough() {
        let machine = utf8_machine();
        assert_eq!(
            machine.encode_output("café\n"),
            "café\r\n".as_bytes().to_vec()
        );
    }

    #[test]
    fn legacy_client_gets_ascii_transliteration() {
        let machine = TelnetMachine::new(); // CHARSET never accepted
        assert_eq!(machine.encode_output("café\n"), b"cafe\r\n".to_vec());
    }

    #[test]
    fn ascii_transliteration_preserves_ansi_escapes() {
        let machine = TelnetMachine::new(); // CHARSET never accepted → ASCII mode
        assert_eq!(
            machine.encode_output("\u{1b}[97mcafé\u{1b}[0m\n"),
            b"\x1b[97mcafe\x1b[0m\r\n".to_vec()
        );
    }

    #[test]
    fn lf_normalizes_to_crlf_without_doubling() {
        let machine = utf8_machine();
        assert_eq!(machine.encode_output("a\r\nb\n"), b"a\r\nb\r\n".to_vec());
    }

    #[test]
    fn prompt_frame_is_ga_by_default() {
        let machine = TelnetMachine::new();
        assert_eq!(machine.prompt_frame(), vec![IAC, GA]);
    }

    #[test]
    fn prompt_frame_is_eor_after_do_eor() {
        let mut machine = TelnetMachine::new();
        let _ = machine.receive(&[IAC, DO, OPT_EOR]);
        assert_eq!(machine.prompt_frame(), vec![IAC, EOR_CMD]);
    }

    #[test]
    fn prompt_frame_falls_back_to_ga_after_dont_eor() {
        let mut machine = TelnetMachine::new();
        let _ = machine.receive(&[IAC, DONT, OPT_EOR]);
        assert_eq!(machine.prompt_frame(), vec![IAC, GA]);
    }

    #[test]
    fn set_echo_queues_the_negotiation_bytes() {
        let mut machine = TelnetMachine::new();
        let _ = machine.take_output(); // discard opening offers
        machine.set_echo(LocalEcho::Suppressed);
        assert_eq!(machine.take_output(), vec![255, 251, 1], "IAC WILL ECHO");
        machine.set_echo(LocalEcho::Enabled);
        assert_eq!(machine.take_output(), vec![255, 252, 1], "IAC WONT ECHO");
    }

    #[test]
    fn a_masked_line_is_answered_with_a_crlf() {
        let mut machine = TelnetMachine::new();
        let _ = machine.take_output();
        machine.set_echo(LocalEcho::Suppressed);
        let _ = machine.receive(&[255, 253, 1]); // client agrees: IAC DO ECHO
        let _ = machine.take_output();
        let events = machine.receive(b"hunter2\r\n");
        assert_eq!(events, vec![TelnetEvent::Line("hunter2".into())]);
        assert_eq!(
            machine.take_output(),
            b"\r\n".to_vec(),
            "the client echoes nothing, so the server advances the line"
        );
    }

    #[test]
    fn an_unmasked_line_gets_no_crlf() {
        let mut machine = TelnetMachine::new();
        let _ = machine.take_output();
        let events = machine.receive(b"look\r\n");
        assert_eq!(events, vec![TelnetEvent::Line("look".into())]);
        assert!(machine.take_output().is_empty());
    }

    #[test]
    fn a_refusing_client_gets_no_crlf_compensation() {
        // The client refused (or ignored) WILL ECHO: it is still echoing
        // locally, including the newline, so no compensation is owed.
        let mut machine = TelnetMachine::new();
        let _ = machine.take_output();
        machine.set_echo(LocalEcho::Suppressed);
        let _ = machine.receive(&[255, 254, 1]); // IAC DONT ECHO
        let _ = machine.take_output();
        let _ = machine.receive(b"visible\r\n");
        assert!(machine.take_output().is_empty());
    }
}
