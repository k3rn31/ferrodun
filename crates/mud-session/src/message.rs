//! Typed, render-agnostic output from the FSM (§3.20): the driver localizes
//! these through `mud-i18n`, so the machine stays free of message strings.

/// A message the FSM asks the driver to present to the session.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SessionMessage {
    /// The tenant-authored welcome banner (§3.19.1).
    Banner,
    /// The pre-login prompt: how to register and how to log in.
    Prompt,
}
