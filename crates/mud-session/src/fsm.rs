//! The login state machine and its transitions.

use crate::effect::{Effect, EffectResult};
use crate::message::SessionMessage;

/// The result of one FSM step: what to show, what I/O to perform, whether the
/// session has left the login flow.
#[derive(Debug)]
#[must_use]
pub struct Transition {
    /// Messages to render to the session, in order.
    pub messages: Vec<SessionMessage>,
    /// An effect the driver must perform, if any. Feed its result back via
    /// [`SessionFsm::on_effect`].
    pub effect: Option<Effect>,
    /// Set once the session leaves the login flow.
    pub terminal: Option<Terminal>,
}

impl Transition {
    fn messages(messages: Vec<SessionMessage>) -> Self {
        Self { messages, effect: None, terminal: None }
    }
}

/// How a login flow ended.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Terminal {
    /// The session's connection should be closed (`quit`).
    Closed,
}

/// A single session's login state machine.
#[derive(Debug)]
#[must_use]
pub struct SessionFsm {
    state: State,
}

#[derive(Debug)]
enum State {
    Anon,
}

impl Default for SessionFsm {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionFsm {
    /// A fresh machine for a just-connected session, before any input.
    pub fn new() -> Self {
        Self { state: State::Anon }
    }

    /// The banner and prompt to present the moment a session connects (§3.19.1).
    pub fn on_connect(&self) -> Transition {
        Transition::messages(vec![SessionMessage::Banner, SessionMessage::Prompt])
    }

    /// Feeds one input line to the machine.
    pub fn on_input(&mut self, _line: &str) -> Transition {
        let _ = &self.state;
        Transition::messages(Vec::new())
    }

    /// Feeds an [`EffectResult`] back after the driver performed an [`Effect`].
    pub fn on_effect(&mut self, _result: EffectResult) -> Transition {
        Transition::messages(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_connect_presents_banner_then_prompt() {
        let fsm = SessionFsm::new();
        let t = fsm.on_connect();
        assert_eq!(t.messages, vec![SessionMessage::Banner, SessionMessage::Prompt]);
        assert!(t.effect.is_none());
        assert!(t.terminal.is_none());
    }
}
