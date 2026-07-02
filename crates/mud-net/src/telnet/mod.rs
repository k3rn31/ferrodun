//! Telnet protocol core (SPEC §2.8.2, M1 subset).
//!
//! Sans-IO: bytes in, typed events plus reply bytes out. The gateway (M1-21)
//! owns the socket and drives this state machine.

mod line;
mod negotiation;
mod parser;
