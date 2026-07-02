//! Incremental telnet IAC framing (RFC 854).
//!
//! Splits a raw byte stream into data bytes, standalone commands, option
//! negotiations, and subnegotiation payloads. State survives across `push`
//! calls, so arbitrary packet splits are handled. Malformed sequences are
//! tolerated: garbage degrades, it never aborts the stream.

pub(crate) const IAC: u8 = 255;
pub(crate) const DONT: u8 = 254;
pub(crate) const DO: u8 = 253;
pub(crate) const WONT: u8 = 252;
pub(crate) const WILL: u8 = 251;
pub(crate) const SB: u8 = 250;
pub(crate) const GA: u8 = 249;
pub(crate) const SE: u8 = 240;
pub(crate) const EOR_CMD: u8 = 239;

/// Cap on a single subnegotiation payload (untrusted input). The largest M1
/// payload is a TTYPE terminal name; 1 KiB is generous.
pub(crate) const MAX_SUBNEG_BYTES: usize = 1024;

/// A negotiation verb following IAC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verb {
    Will,
    Wont,
    Do,
    Dont,
}

/// One decoded item from the telnet stream.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ParsedItem {
    /// A data byte (IAC IAC already unescaped).
    Data(u8),
    /// A standalone command byte, e.g. GA or NOP.
    Command(u8),
    /// An option negotiation: IAC <verb> <option>.
    Negotiate { verb: Verb, option: u8 },
    /// A complete subnegotiation: IAC SB <option> <payload> IAC SE,
    /// with IAC IAC in the payload already unescaped.
    Subnegotiation { option: u8, payload: Vec<u8> },
}

#[derive(Debug)]
enum State {
    Data,
    Iac,
    Verb(Verb),
    SubOption,
    SubPayload { option: u8, payload: Vec<u8>, overflowed: bool },
    SubIac { option: u8, payload: Vec<u8>, overflowed: bool },
}

/// Incremental IAC parser; one per connection.
#[derive(Debug)]
pub(crate) struct IacParser {
    state: State,
}

impl IacParser {
    pub(crate) fn new() -> Self {
        Self { state: State::Data }
    }

    /// Consumes raw socket bytes, returning the items completed by them.
    pub(crate) fn push(&mut self, bytes: &[u8]) -> Vec<ParsedItem> {
        let mut items = Vec::new();
        for &byte in bytes {
            self.step(byte, &mut items);
        }
        items
    }

    fn step(&mut self, byte: u8, items: &mut Vec<ParsedItem>) {
        // Take ownership of the state so subnegotiation payloads move, not clone.
        let state = std::mem::replace(&mut self.state, State::Data);
        self.state = match state {
            State::Data => match byte {
                IAC => State::Iac,
                data => {
                    items.push(ParsedItem::Data(data));
                    State::Data
                }
            },
            State::Iac => Self::after_iac(byte, items),
            State::Verb(verb) => {
                items.push(ParsedItem::Negotiate { verb, option: byte });
                State::Data
            }
            State::SubOption => State::SubPayload { option: byte, payload: Vec::new(), overflowed: false },
            State::SubPayload { option, payload, overflowed } => {
                if byte == IAC {
                    State::SubIac { option, payload, overflowed }
                } else {
                    Self::sub_push(option, payload, overflowed, byte)
                }
            }
            State::SubIac { option, payload, overflowed } => match byte {
                SE => {
                    if !overflowed {
                        items.push(ParsedItem::Subnegotiation { option, payload });
                    }
                    State::Data
                }
                IAC => Self::sub_push(option, payload, overflowed, IAC),
                // Malformed: IAC <non-SE> inside SB. Drop the subnegotiation
                // and reinterpret the byte as if it followed a fresh IAC.
                other => Self::after_iac(other, items),
            },
        };
    }

    fn after_iac(byte: u8, items: &mut Vec<ParsedItem>) -> State {
        match byte {
            IAC => {
                items.push(ParsedItem::Data(IAC));
                State::Data
            }
            WILL => State::Verb(Verb::Will),
            WONT => State::Verb(Verb::Wont),
            DO => State::Verb(Verb::Do),
            DONT => State::Verb(Verb::Dont),
            SB => State::SubOption,
            command => {
                items.push(ParsedItem::Command(command));
                State::Data
            }
        }
    }

    fn sub_push(option: u8, mut payload: Vec<u8>, overflowed: bool, byte: u8) -> State {
        let overflowed = overflowed || payload.len() >= MAX_SUBNEG_BYTES;
        if !overflowed {
            payload.push(byte);
        }
        State::SubPayload { option, payload, overflowed }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_data_passes_through() {
        let mut parser = IacParser::new();
        let items = parser.push(b"hi");
        assert_eq!(items, vec![ParsedItem::Data(b'h'), ParsedItem::Data(b'i')]);
    }

    #[test]
    fn iac_iac_unescapes_to_data_255() {
        let mut parser = IacParser::new();
        let items = parser.push(&[IAC, IAC]);
        assert_eq!(items, vec![ParsedItem::Data(IAC)]);
    }

    #[test]
    fn negotiation_verbs_parse() {
        let mut parser = IacParser::new();
        let items = parser.push(&[IAC, WILL, 31, IAC, WONT, 24, IAC, DO, 25, IAC, DONT, 42]);
        assert_eq!(
            items,
            vec![
                ParsedItem::Negotiate { verb: Verb::Will, option: 31 },
                ParsedItem::Negotiate { verb: Verb::Wont, option: 24 },
                ParsedItem::Negotiate { verb: Verb::Do, option: 25 },
                ParsedItem::Negotiate { verb: Verb::Dont, option: 42 },
            ]
        );
    }

    #[test]
    fn bare_command_parses() {
        let mut parser = IacParser::new();
        let items = parser.push(&[IAC, GA]);
        assert_eq!(items, vec![ParsedItem::Command(GA)]);
    }

    #[test]
    fn subnegotiation_collects_payload() {
        let mut parser = IacParser::new();
        // IAC SB NAWS 0 80 0 24 IAC SE
        let items = parser.push(&[IAC, SB, 31, 0, 80, 0, 24, IAC, SE]);
        assert_eq!(
            items,
            vec![ParsedItem::Subnegotiation { option: 31, payload: vec![0, 80, 0, 24] }]
        );
    }

    #[test]
    fn subnegotiation_unescapes_iac_iac_in_payload() {
        let mut parser = IacParser::new();
        let items = parser.push(&[IAC, SB, 31, IAC, IAC, 7, IAC, SE]);
        assert_eq!(
            items,
            vec![ParsedItem::Subnegotiation { option: 31, payload: vec![IAC, 7] }]
        );
    }

    #[test]
    fn state_survives_split_packets() {
        let mut parser = IacParser::new();
        // Split mid-command and mid-subnegotiation across four pushes.
        let mut items = parser.push(&[IAC]);
        items.extend(parser.push(&[SB, 31]));
        items.extend(parser.push(&[0, 80]));
        items.extend(parser.push(&[0, 24, IAC, SE]));
        assert_eq!(
            items,
            vec![ParsedItem::Subnegotiation { option: 31, payload: vec![0, 80, 0, 24] }]
        );
    }

    #[test]
    fn oversized_subnegotiation_is_discarded_and_parser_recovers() {
        let mut parser = IacParser::new();
        let mut input = vec![IAC, SB, 31];
        input.extend(std::iter::repeat_n(b'x', MAX_SUBNEG_BYTES + 1));
        input.extend([IAC, SE]);
        input.extend(b"ok");
        let items = parser.push(&input);
        // The oversized subnegotiation yields nothing; parsing resumes after it.
        assert_eq!(items, vec![ParsedItem::Data(b'o'), ParsedItem::Data(b'k')]);
    }

    #[test]
    fn malformed_iac_inside_subnegotiation_drops_it_and_resyncs() {
        let mut parser = IacParser::new();
        // IAC WILL inside a subnegotiation: drop the subnegotiation, honor the
        // negotiation, keep parsing.
        let items = parser.push(&[IAC, SB, 31, 1, 2, IAC, WILL, 24, b'a']);
        assert_eq!(
            items,
            vec![
                ParsedItem::Negotiate { verb: Verb::Will, option: 24 },
                ParsedItem::Data(b'a'),
            ]
        );
    }
}
