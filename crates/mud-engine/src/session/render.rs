//! Renders a [`SessionMessage`] to the plain text a session receives (§3.20.5).
//!
//! Pre-login output has no account locale, so the driver renders at the tenant
//! default locale. The banner is tenant-authored KDL (§3.19.1), supplied here
//! rather than looked up in the catalog. Styled output is deferred (M1 uses
//! plain text at the IPC boundary, §3.20 note); this returns `String`.

use mud_i18n::{Locale, t};
use mud_schema::OutputKind;
use mud_session::SessionMessage;

/// Classifies a message's output block (§2.8.2 line discipline): the two
/// password prompts leave the cursor on their line; every other message is a
/// completed line the gateway terminates. Exhaustive so a new variant forces
/// a classification decision here.
pub(crate) fn kind(message: &SessionMessage) -> OutputKind {
    match message {
        SessionMessage::PasswordPrompt | SessionMessage::ConfirmPrompt => OutputKind::Prompt,
        SessionMessage::Banner
        | SessionMessage::LoginInstructions
        | SessionMessage::PreLoginHelp
        | SessionMessage::WhoStub
        | SessionMessage::UnknownCommand
        | SessionMessage::Goodbye
        | SessionMessage::LoginFailed
        | SessionMessage::AccountSuspended
        | SessionMessage::AccountBanned
        | SessionMessage::ServerError
        | SessionMessage::PuppetList(_)
        | SessionMessage::NoPuppetsYet
        | SessionMessage::NoSuchPuppet
        | SessionMessage::PasswordMismatch
        | SessionMessage::NameInvalid
        | SessionMessage::UsernameTaken
        | SessionMessage::PuppetCreated(_)
        | SessionMessage::EnteredWorld => OutputKind::Line,
    }
}

/// Renders `message` to plain text at `locale`, using `banner` for the welcome.
pub(crate) fn render(message: &SessionMessage, banner: &str, locale: &Locale) -> String {
    match message {
        SessionMessage::Banner => banner.to_owned(),
        SessionMessage::LoginInstructions => t!(*locale, "session.prompt"),
        SessionMessage::PreLoginHelp => t!(*locale, "session.help"),
        SessionMessage::WhoStub => t!(*locale, "session.who-stub"),
        SessionMessage::UnknownCommand => t!(*locale, "session.unknown"),
        SessionMessage::PasswordPrompt => t!(*locale, "session.password"),
        SessionMessage::ConfirmPrompt => t!(*locale, "session.confirm"),
        SessionMessage::LoginFailed => t!(*locale, "session.login-failed"),
        SessionMessage::AccountSuspended => t!(*locale, "session.suspended"),
        SessionMessage::AccountBanned => t!(*locale, "session.banned"),
        SessionMessage::ServerError => t!(*locale, "session.server-error"),
        SessionMessage::NoPuppetsYet => t!(*locale, "session.no-puppets"),
        SessionMessage::NoSuchPuppet => t!(*locale, "session.no-such-puppet"),
        SessionMessage::PasswordMismatch => t!(*locale, "session.mismatch"),
        SessionMessage::NameInvalid => t!(*locale, "session.name-invalid"),
        SessionMessage::UsernameTaken => t!(*locale, "session.username-taken"),
        SessionMessage::EnteredWorld => t!(*locale, "session.entered"),
        SessionMessage::Goodbye => t!(*locale, "session.goodbye"),
        SessionMessage::PuppetList(names) => {
            let names = names
                .iter()
                .enumerate()
                .map(|(i, n)| format!("  {}) {}", i + 1, n.as_str()))
                .collect::<Vec<_>>()
                .join("\n");
            t!(*locale, "session.puppet-list", names = names)
        }
        SessionMessage::PuppetCreated(name) => {
            t!(*locale, "session.puppet-created", name = name.as_str())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mud_account::PuppetName;
    use mud_schema::OutputKind;

    #[test]
    fn password_prompts_classify_as_prompt_blocks() {
        assert_eq!(kind(&SessionMessage::PasswordPrompt), OutputKind::Prompt);
        assert_eq!(kind(&SessionMessage::ConfirmPrompt), OutputKind::Prompt);
    }

    #[test]
    fn ordinary_messages_classify_as_line_blocks() {
        for message in [
            SessionMessage::Banner,
            SessionMessage::LoginInstructions,
            SessionMessage::LoginFailed,
            SessionMessage::EnteredWorld,
        ] {
            assert_eq!(kind(&message), OutputKind::Line, "for {message:?}");
        }
    }

    #[test]
    fn banner_message_renders_the_supplied_banner() {
        assert_eq!(
            render(&SessionMessage::Banner, "WELCOME", &Locale::EN),
            "WELCOME"
        );
    }

    #[test]
    fn prompt_renders_from_the_catalog() {
        let text = render(&SessionMessage::LoginInstructions, "", &Locale::EN);
        assert!(text.contains("login"), "unexpected prompt: {text}");
    }

    #[test]
    fn puppet_list_renders_a_numbered_menu() {
        let names = vec![
            PuppetName::parse("arden").expect("name"),
            PuppetName::parse("borel").expect("name"),
        ];
        let text = render(&SessionMessage::PuppetList(names), "", &Locale::EN);
        assert_eq!(
            text,
            "Your characters:\n  1) arden\n  2) borel\nType 'play <name or number>' or 'new <name>'."
        );
    }

    #[test]
    fn no_such_puppet_renders_from_the_catalog() {
        assert_eq!(kind(&SessionMessage::NoSuchPuppet), OutputKind::Line);
        assert_eq!(
            render(&SessionMessage::NoSuchPuppet, "", &Locale::EN),
            "No such character."
        );
    }
}
