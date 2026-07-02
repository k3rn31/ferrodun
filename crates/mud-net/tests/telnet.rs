//! End-to-end telnet negotiation flows through the public `mud-net` API.

use mud_net::{TelnetEvent, TelnetMachine};

const IAC: u8 = 255;
const DONT: u8 = 254;
const DO: u8 = 253;
const WONT: u8 = 252;
const WILL: u8 = 251;
const SB: u8 = 250;
const GA: u8 = 249;
const SE: u8 = 240;
const EOR_CMD: u8 = 239;
const OPT_TTYPE: u8 = 24;
const OPT_EOR: u8 = 25;
const OPT_NAWS: u8 = 31;
const OPT_CHARSET: u8 = 42;
const TTYPE_IS: u8 = 0;
const CHARSET_ACCEPTED: u8 = 2;

#[test]
fn modern_client_full_session() {
    let mut machine = TelnetMachine::new();

    // Server opens with its offers.
    let offers = machine.take_output();
    assert_eq!(
        offers,
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

    // Client accepts everything.
    let events = machine.receive(&[
        IAC,
        WILL,
        OPT_NAWS,
        IAC,
        WILL,
        OPT_TTYPE,
        IAC,
        DO,
        OPT_EOR,
        IAC,
        DO,
        OPT_CHARSET,
    ]);
    assert!(events.is_empty(), "pure negotiation produces no events");

    // Server responds with TTYPE SEND and the CHARSET UTF-8 request.
    let replies = machine.take_output();
    let mut expected = vec![IAC, SB, OPT_TTYPE, 1, IAC, SE, IAC, SB, OPT_CHARSET, 1];
    expected.extend_from_slice(b";UTF-8");
    expected.extend_from_slice(&[IAC, SE]);
    assert_eq!(replies, expected);

    // Client answers both subnegotiations.
    let mut input = vec![IAC, SB, OPT_TTYPE, TTYPE_IS];
    input.extend_from_slice(b"MUDLET");
    input.extend_from_slice(&[IAC, SE, IAC, SB, OPT_CHARSET, CHARSET_ACCEPTED]);
    input.extend_from_slice(b"UTF-8");
    input.extend_from_slice(&[IAC, SE, IAC, SB, OPT_NAWS, 0, 120, 0, 40, IAC, SE]);
    let events = machine.receive(&input);
    assert_eq!(
        events,
        vec![
            TelnetEvent::TerminalType("MUDLET".to_owned()),
            TelnetEvent::WindowSize {
                width: 120,
                height: 40
            },
        ]
    );

    // A command flows through; output is UTF-8; prompts are EOR-framed.
    let events = machine.receive(b"say caf\xC3\xA9\r\n");
    assert_eq!(events, vec![TelnetEvent::Line("say café".to_owned())]);
    assert_eq!(
        machine.encode_output("café\n"),
        "café\r\n".as_bytes().to_vec()
    );
    assert_eq!(machine.prompt_frame(), vec![IAC, EOR_CMD]);
}

#[test]
fn legacy_client_refuses_everything() {
    let mut machine = TelnetMachine::new();
    let _ = machine.take_output();

    let events = machine.receive(&[
        IAC,
        WONT,
        OPT_NAWS,
        IAC,
        WONT,
        OPT_TTYPE,
        IAC,
        DONT,
        OPT_EOR,
        IAC,
        DONT,
        OPT_CHARSET,
    ]);
    assert!(events.is_empty());
    assert!(
        machine.take_output().is_empty(),
        "refusals of pending offers need no replies"
    );

    // Commands still work; output is transliterated; prompts are GA-framed.
    let events = machine.receive(b"look\r\n");
    assert_eq!(events, vec![TelnetEvent::Line("look".to_owned())]);
    assert_eq!(machine.encode_output("café\n"), b"cafe\r\n".to_vec());
    assert_eq!(machine.prompt_frame(), vec![IAC, GA]);
}
