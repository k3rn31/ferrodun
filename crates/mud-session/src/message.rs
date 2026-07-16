//! Typed, render-agnostic output from the FSM (§3.20): the driver localizes
//! these through `mud-i18n`, so the machine stays free of message strings.

/// A message the FSM asks the driver to present to the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionMessage {
    /// The tenant-authored welcome banner (§3.19.1).
    Banner,
    /// The pre-login instruction line: how to register and how to log in.
    LoginInstructions,
    /// The pre-login help listing (`help` / `?`, §3.19.1).
    PreLoginHelp,
    /// The pre-login `who` stub (real listing lands in M1-19a).
    WhoStub,
    /// The player typed something unrecognized at the pre-login prompt.
    UnknownCommand,
    /// A farewell shown before the session closes.
    Goodbye,
    /// Prompt for a password on its own line. Echo suppression rides on
    /// [`Transition::echo`](crate::Transition), not on this message.
    PasswordPrompt,
    /// A non-leaky login failure: wrong user *or* wrong password read alike.
    LoginFailed,
    /// The account is suspended (§3.15.1.5).
    AccountSuspended,
    /// The account is banned (§3.15.1.5).
    AccountBanned,
    /// A server-side fault; the player may retry.
    ServerError,
    /// The account's puppets, offered for selection.
    PuppetList(Vec<mud_account::PuppetName>),
    /// The account owns no puppets yet; prompt to create the first.
    NoPuppetsYet,
    /// Prompt to re-enter the password during registration.
    ConfirmPrompt,
    /// The two registration passwords did not match.
    PasswordMismatch,
    /// The requested name is not a valid account/puppet name.
    NameInvalid,
    /// The requested username is already taken in this tenant.
    UsernameTaken,
    /// A puppet was created; echoes its name.
    PuppetCreated(mud_account::PuppetName),
    /// The player has entered the world.
    EnteredWorld,
}
