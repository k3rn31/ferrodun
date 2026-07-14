//! Telnet option negotiation for the M1 subset (SPEC §2.8.2).
//!
//! RFC 1143 Q-method state per option prevents negotiation loops. Options we
//! ask the client to enable ("him"): NAWS (RFC 1073), TTYPE (RFC 1091).
//! Options we offer to enable ourselves ("us"): EOR (RFC 885), CHARSET
//! (RFC 2066, UTF-8 only). ECHO is server-claimed around password entry
//! (RFC 857); SGA and everything else is refused; unknown options are never
//! silently ignored.

use super::parser::{DO, DONT, IAC, SB, SE, Verb, WILL, WONT};

pub(crate) const OPT_ECHO: u8 = 1;
pub(crate) const OPT_TTYPE: u8 = 24;
pub(crate) const OPT_EOR: u8 = 25;
pub(crate) const OPT_NAWS: u8 = 31;
pub(crate) const OPT_CHARSET: u8 = 42;

pub(crate) const TTYPE_IS: u8 = 0;
pub(crate) const TTYPE_SEND: u8 = 1;
pub(crate) const CHARSET_REQUEST: u8 = 1;
pub(crate) const CHARSET_ACCEPTED: u8 = 2;
pub(crate) const CHARSET_REJECTED: u8 = 3;

/// Server→client text encoding, decided by CHARSET negotiation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CharsetMode {
    /// Legacy client: transliterate to ASCII.
    Ascii,
    /// Client accepted UTF-8 via CHARSET.
    Utf8,
}

/// RFC 1143 Q-method option state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QState {
    No,
    WantYes,
    Yes,
    /// We sent a disabling verb and await the acknowledgement.
    WantNo,
}

/// Per-connection negotiation state for the M1 option subset.
#[derive(Debug)]
pub(crate) struct Negotiator {
    him_naws: QState,
    him_ttype: QState,
    us_eor: QState,
    us_charset: QState,
    us_echo: QState,
    charset_mode: CharsetMode,
}

impl Negotiator {
    /// Creates the negotiator and queues the opening offers into `out`:
    /// DO NAWS, DO TTYPE, WILL EOR, WILL CHARSET.
    pub(crate) fn new(out: &mut Vec<u8>) -> Self {
        out.extend_from_slice(&[
            IAC,
            DO,
            OPT_NAWS,
            IAC,
            DO,
            OPT_TTYPE,
            IAC,
            WILL,
            OPT_EOR,
            IAC,
            WILL,
            OPT_CHARSET,
        ]);
        Self {
            him_naws: QState::WantYes,
            him_ttype: QState::WantYes,
            us_eor: QState::WantYes,
            us_charset: QState::WantYes,
            us_echo: QState::No,
            charset_mode: CharsetMode::Ascii,
        }
    }

    /// Handles one IAC negotiation, queueing any required reply into `out`.
    pub(crate) fn on_negotiate(&mut self, verb: Verb, option: u8, out: &mut Vec<u8>) {
        match verb {
            Verb::Will => match option {
                OPT_NAWS => {
                    let _ = Self::enable(&mut self.him_naws, DO, option, out);
                }
                OPT_TTYPE => {
                    if Self::enable(&mut self.him_ttype, DO, option, out) {
                        out.extend_from_slice(&[IAC, SB, OPT_TTYPE, TTYPE_SEND, IAC, SE]);
                    }
                }
                unsupported => out.extend_from_slice(&[IAC, DONT, unsupported]),
            },
            Verb::Wont => match option {
                OPT_NAWS => Self::disable(&mut self.him_naws, DONT, option, out),
                OPT_TTYPE => Self::disable(&mut self.him_ttype, DONT, option, out),
                _ => {}
            },
            Verb::Do => match option {
                OPT_EOR => {
                    let _ = Self::enable(&mut self.us_eor, WILL, option, out);
                }
                OPT_CHARSET => {
                    if Self::enable(&mut self.us_charset, WILL, option, out) {
                        out.extend_from_slice(&[IAC, SB, OPT_CHARSET, CHARSET_REQUEST]);
                        out.extend_from_slice(b";UTF-8");
                        out.extend_from_slice(&[IAC, SE]);
                    }
                }
                OPT_ECHO => match self.us_echo {
                    QState::WantYes => self.us_echo = QState::Yes,
                    // No offer outstanding: the server never echoes normal
                    // input, so a spontaneous DO is refused.
                    QState::No => out.extend_from_slice(&[IAC, WONT, OPT_ECHO]),
                    // Stale agreement to a WILL we have since retracted; our
                    // WONT is in flight and the client's DONT lands us in No.
                    QState::WantNo => {}
                    QState::Yes => {}
                },
                unsupported => out.extend_from_slice(&[IAC, WONT, unsupported]),
            },
            Verb::Dont => match option {
                OPT_EOR => Self::disable(&mut self.us_eor, WONT, option, out),
                OPT_CHARSET => Self::disable(&mut self.us_charset, WONT, option, out),
                OPT_ECHO => Self::disable(&mut self.us_echo, WONT, option, out),
                _ => {}
            },
        }
    }

    /// Handles a CHARSET subnegotiation payload: ACCEPTED switches to UTF-8,
    /// REJECTED keeps ASCII transliteration. Any other payload is ignored.
    pub(crate) fn on_charset_subnegotiation(&mut self, payload: &[u8]) {
        match payload.split_first() {
            Some((&CHARSET_ACCEPTED, _)) => self.charset_mode = CharsetMode::Utf8,
            Some((&CHARSET_REJECTED, _)) => self.charset_mode = CharsetMode::Ascii,
            _ => {}
        }
    }

    /// True when EOR framing was negotiated; otherwise prompts use GA.
    pub(crate) fn eor_enabled(&self) -> bool {
        self.us_eor == QState::Yes
    }

    /// Current server→client text encoding.
    pub(crate) fn charset(&self) -> CharsetMode {
        self.charset_mode
    }

    /// RFC 1143: agreement transition. Replies with `ack_verb` only when the
    /// remote initiated (state `No`); a reply to our own pending offer would
    /// loop. Returns true when the option became newly enabled.
    fn enable(state: &mut QState, ack_verb: u8, option: u8, out: &mut Vec<u8>) -> bool {
        match *state {
            QState::WantYes => {
                *state = QState::Yes;
                true
            }
            QState::No | QState::WantNo => {
                *state = QState::Yes;
                out.extend_from_slice(&[IAC, ack_verb, option]);
                true
            }
            QState::Yes => false,
        }
    }

    /// RFC 1143: refusal transition. Acknowledges with `ack_verb` only when
    /// leaving `Yes`; a refusal of a pending offer needs no reply.
    fn disable(state: &mut QState, ack_verb: u8, option: u8, out: &mut Vec<u8>) {
        match *state {
            QState::Yes => {
                *state = QState::No;
                out.extend_from_slice(&[IAC, ack_verb, option]);
            }
            QState::WantYes | QState::WantNo | QState::No => *state = QState::No,
        }
    }

    /// Claims the ECHO option (RFC 857): asks the client to stop local echo
    /// for password entry. Idempotent while an offer is pending or active.
    pub(crate) fn suppress_echo(&mut self, out: &mut Vec<u8>) {
        match self.us_echo {
            QState::No | QState::WantNo => {
                self.us_echo = QState::WantYes;
                out.extend_from_slice(&[IAC, WILL, OPT_ECHO]);
            }
            QState::WantYes | QState::Yes => {}
        }
    }

    /// Releases the ECHO option: the client resumes local echo. Also sent
    /// from `WantYes` — the password line can be consumed before the
    /// client's DO arrives, and the retraction must still go out.
    pub(crate) fn restore_echo(&mut self, out: &mut Vec<u8>) {
        match self.us_echo {
            QState::Yes | QState::WantYes => {
                self.us_echo = QState::WantNo;
                out.extend_from_slice(&[IAC, WONT, OPT_ECHO]);
            }
            QState::WantNo | QState::No => {}
        }
    }

    /// True when the client has agreed to suppress its local echo.
    pub(crate) fn echo_suppressed(&self) -> bool {
        self.us_echo == QState::Yes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opened() -> (Negotiator, Vec<u8>) {
        let mut out = Vec::new();
        let negotiator = Negotiator::new(&mut out);
        out.clear(); // discard opening offers; tests below assert replies only
        (negotiator, out)
    }

    #[test]
    fn opening_offers_are_queued() {
        let mut out = Vec::new();
        let _negotiator = Negotiator::new(&mut out);
        assert_eq!(
            out,
            vec![
                IAC,
                DO,
                OPT_NAWS,
                IAC,
                DO,
                OPT_TTYPE,
                IAC,
                WILL,
                OPT_EOR,
                IAC,
                WILL,
                OPT_CHARSET,
            ]
        );
    }

    #[test]
    fn will_naws_after_our_do_needs_no_reply() {
        let (mut negotiator, mut out) = opened();
        negotiator.on_negotiate(Verb::Will, OPT_NAWS, &mut out);
        assert!(
            out.is_empty(),
            "WILL answering our DO must not be re-acknowledged"
        );
    }

    #[test]
    fn repeated_will_naws_does_not_loop() {
        let (mut negotiator, mut out) = opened();
        negotiator.on_negotiate(Verb::Will, OPT_NAWS, &mut out);
        negotiator.on_negotiate(Verb::Will, OPT_NAWS, &mut out);
        assert!(
            out.is_empty(),
            "already-enabled option must be ignored (RFC 1143)"
        );
    }

    #[test]
    fn will_ttype_triggers_send_request() {
        let (mut negotiator, mut out) = opened();
        negotiator.on_negotiate(Verb::Will, OPT_TTYPE, &mut out);
        assert_eq!(out, vec![IAC, SB, OPT_TTYPE, TTYPE_SEND, IAC, SE]);
    }

    #[test]
    fn do_eor_enables_eor_framing() {
        let (mut negotiator, mut out) = opened();
        assert!(!negotiator.eor_enabled());
        negotiator.on_negotiate(Verb::Do, OPT_EOR, &mut out);
        assert!(negotiator.eor_enabled());
        assert!(
            out.is_empty(),
            "DO answering our WILL must not be re-acknowledged"
        );
    }

    #[test]
    fn dont_eor_leaves_ga_framing() {
        let (mut negotiator, mut out) = opened();
        negotiator.on_negotiate(Verb::Dont, OPT_EOR, &mut out);
        assert!(!negotiator.eor_enabled());
        assert!(out.is_empty(), "refusal of a pending offer needs no reply");
    }

    #[test]
    fn do_charset_triggers_utf8_request() {
        let (mut negotiator, mut out) = opened();
        negotiator.on_negotiate(Verb::Do, OPT_CHARSET, &mut out);
        let mut expected = vec![IAC, SB, OPT_CHARSET, CHARSET_REQUEST];
        expected.extend_from_slice(b";UTF-8");
        expected.extend_from_slice(&[IAC, SE]);
        assert_eq!(out, expected);
    }

    #[test]
    fn charset_accepted_switches_to_utf8() {
        let (mut negotiator, mut out) = opened();
        negotiator.on_negotiate(Verb::Do, OPT_CHARSET, &mut out);
        assert_eq!(negotiator.charset(), CharsetMode::Ascii);
        let mut payload = vec![CHARSET_ACCEPTED];
        payload.extend_from_slice(b"UTF-8");
        negotiator.on_charset_subnegotiation(&payload);
        assert_eq!(negotiator.charset(), CharsetMode::Utf8);
    }

    #[test]
    fn charset_rejected_stays_ascii() {
        let (mut negotiator, mut out) = opened();
        negotiator.on_negotiate(Verb::Do, OPT_CHARSET, &mut out);
        negotiator.on_charset_subnegotiation(&[CHARSET_REJECTED]);
        assert_eq!(negotiator.charset(), CharsetMode::Ascii);
    }

    #[test]
    fn unknown_will_is_refused_with_dont() {
        let (mut negotiator, mut out) = opened();
        negotiator.on_negotiate(Verb::Will, 86, &mut out); // MCCP2, out of M1 scope
        assert_eq!(out, vec![IAC, DONT, 86]);
    }

    #[test]
    fn do_echo_is_refused_with_wont() {
        let (mut negotiator, mut out) = opened();
        negotiator.on_negotiate(Verb::Do, 1, &mut out); // ECHO: the server never echoes normal input
        assert_eq!(out, vec![IAC, WONT, 1]);
    }

    #[test]
    fn suppress_echo_sends_will_echo() {
        let (mut negotiator, mut out) = opened();
        negotiator.suppress_echo(&mut out);
        assert_eq!(out, vec![IAC, WILL, OPT_ECHO]);
    }

    #[test]
    fn suppress_echo_twice_sends_one_will() {
        let (mut negotiator, mut out) = opened();
        negotiator.suppress_echo(&mut out);
        out.clear();
        negotiator.suppress_echo(&mut out);
        assert!(out.is_empty(), "a pending offer must not repeat");
    }

    #[test]
    fn do_echo_after_our_will_enables_without_reply() {
        let (mut negotiator, mut out) = opened();
        negotiator.suppress_echo(&mut out);
        out.clear();
        negotiator.on_negotiate(Verb::Do, OPT_ECHO, &mut out);
        assert!(negotiator.echo_suppressed());
        assert!(
            out.is_empty(),
            "DO answering our WILL must not be re-acknowledged"
        );
    }

    #[test]
    fn dont_echo_after_our_will_is_a_refusal() {
        let (mut negotiator, mut out) = opened();
        negotiator.suppress_echo(&mut out);
        out.clear();
        negotiator.on_negotiate(Verb::Dont, OPT_ECHO, &mut out);
        assert!(!negotiator.echo_suppressed());
        assert!(out.is_empty(), "refusal of a pending offer needs no reply");
    }

    #[test]
    fn restore_echo_after_agreement_sends_wont_and_dont_acks_it() {
        let (mut negotiator, mut out) = opened();
        negotiator.suppress_echo(&mut out);
        negotiator.on_negotiate(Verb::Do, OPT_ECHO, &mut out);
        out.clear();
        negotiator.restore_echo(&mut out);
        assert_eq!(out, vec![IAC, WONT, OPT_ECHO]);
        assert!(!negotiator.echo_suppressed());
        out.clear();
        negotiator.on_negotiate(Verb::Dont, OPT_ECHO, &mut out);
        assert!(
            out.is_empty(),
            "DONT answering our WONT must not be re-acknowledged"
        );
    }

    #[test]
    fn restore_echo_before_the_client_replied_still_sends_wont() {
        // The password line can be consumed before the client's DO arrives.
        let (mut negotiator, mut out) = opened();
        negotiator.suppress_echo(&mut out);
        out.clear();
        negotiator.restore_echo(&mut out);
        assert_eq!(out, vec![IAC, WONT, OPT_ECHO]);
    }

    #[test]
    fn stale_do_echo_after_our_wont_is_ignored() {
        // WILL sent, WONT sent, then the client's DO (answering the WILL)
        // arrives: our WONT is already in flight and wins.
        let (mut negotiator, mut out) = opened();
        negotiator.suppress_echo(&mut out);
        negotiator.restore_echo(&mut out);
        out.clear();
        negotiator.on_negotiate(Verb::Do, OPT_ECHO, &mut out);
        assert!(out.is_empty());
        assert!(!negotiator.echo_suppressed());
        negotiator.on_negotiate(Verb::Dont, OPT_ECHO, &mut out);
        assert!(out.is_empty(), "the DONT lands us back in No, silently");
    }

    #[test]
    fn client_initiated_will_naws_is_accepted_with_do() {
        // A client that volunteers NAWS after refusing it first: WONT then WILL.
        let (mut negotiator, mut out) = opened();
        negotiator.on_negotiate(Verb::Wont, OPT_NAWS, &mut out);
        assert!(out.is_empty(), "WONT answering a pending DO needs no reply");
        negotiator.on_negotiate(Verb::Will, OPT_NAWS, &mut out);
        assert_eq!(
            out,
            vec![IAC, DO, OPT_NAWS],
            "late client-initiated WILL is accepted"
        );
    }
}
