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

    fn message(message: SessionMessage) -> Self {
        Self::messages(vec![message])
    }

    fn closing(message: SessionMessage) -> Self {
        Self { messages: vec![message], effect: None, terminal: Some(Terminal::Closed) }
    }
}

/// Splits a line into a lowercased command word and the untrimmed remainder.
fn split_command(line: &str) -> Option<(String, &str)> {
    let trimmed = line.trim_start();
    let end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    let (word, rest) = trimmed.split_at(end);
    if word.is_empty() {
        None
    } else {
        Some((word.to_ascii_lowercase(), rest.trim()))
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
    pub fn on_input(&mut self, line: &str) -> Transition {
        match &self.state {
            State::Anon => self.anon_input(line),
        }
    }

    fn anon_input(&mut self, line: &str) -> Transition {
        let Some((command, _rest)) = split_command(line) else {
            return Transition::messages(Vec::new());
        };
        match command.as_str() {
            "help" | "?" => Transition::message(SessionMessage::PreLoginHelp),
            "who" => Transition::message(SessionMessage::WhoStub),
            "quit" => Transition::closing(SessionMessage::Goodbye),
            _ => Transition::message(SessionMessage::UnknownCommand),
        }
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

    #[test]
    fn help_and_question_mark_list_pre_login_commands() {
        for line in ["help", "?", "  HELP  "] {
            let mut fsm = SessionFsm::new();
            let t = fsm.on_input(line);
            assert_eq!(t.messages, vec![SessionMessage::PreLoginHelp], "line: {line:?}");
            assert!(t.terminal.is_none());
        }
    }

    #[test]
    fn who_returns_the_stub() {
        let mut fsm = SessionFsm::new();
        assert_eq!(fsm.on_input("who").messages, vec![SessionMessage::WhoStub]);
    }

    #[test]
    fn quit_says_goodbye_and_closes() {
        let mut fsm = SessionFsm::new();
        let t = fsm.on_input("quit");
        assert_eq!(t.messages, vec![SessionMessage::Goodbye]);
        assert_eq!(t.terminal, Some(Terminal::Closed));
    }

    #[test]
    fn an_unknown_command_reports_and_stays_anon() {
        let mut fsm = SessionFsm::new();
        let t = fsm.on_input("frobnicate");
        assert_eq!(t.messages, vec![SessionMessage::UnknownCommand]);
        assert!(t.terminal.is_none());
    }

    #[test]
    fn blank_input_is_silently_ignored() {
        let mut fsm = SessionFsm::new();
        assert!(fsm.on_input("   ").messages.is_empty());
    }
}
