//! Renders a [`SessionMessage`] to the plain text a session receives (§3.20.5).
//!
//! Pre-login output has no account locale, so the driver renders at the tenant
//! default locale. The banner is tenant-authored KDL (§3.19.1), supplied here
//! rather than looked up in the catalog. Styled output is deferred (M1 uses
//! plain text at the IPC boundary, §3.20 note); this returns `String`.

use mud_i18n::{Locale, t};
use mud_session::SessionMessage;

/// Renders `message` to plain text at `locale`, using `banner` for the welcome.
pub(crate) fn render(message: &SessionMessage, banner: &str, locale: &Locale) -> String {
    match message {
        SessionMessage::Banner => banner.to_owned(),
        SessionMessage::Prompt => t!(*locale, "session.prompt"),
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
        SessionMessage::PasswordMismatch => t!(*locale, "session.mismatch"),
        SessionMessage::NameInvalid => t!(*locale, "session.name-invalid"),
        SessionMessage::UsernameTaken => t!(*locale, "session.username-taken"),
        SessionMessage::EnteredWorld => t!(*locale, "session.entered"),
        SessionMessage::Goodbye => t!(*locale, "session.goodbye"),
        SessionMessage::PuppetList(names) => {
            let list = names.iter().map(|n| n.as_str()).collect::<Vec<_>>().join(", ");
            format!("Your characters: {list}. Type 'play <name>' or 'new <name>'.")
        }
        SessionMessage::PuppetCreated(name) => format!("Created {name}.", name = name.as_str()),
        // `SessionMessage` is `#[non_exhaustive]`; a future variant with no
        // rendering here falls back to the generic server-error text rather
        // than failing to compile against a sibling crate's addition.
        _ => t!(*locale, "session.server-error"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mud_account::PuppetName;

    #[test]
    fn banner_message_renders_the_supplied_banner() {
        assert_eq!(render(&SessionMessage::Banner, "WELCOME", &Locale::EN), "WELCOME");
    }

    #[test]
    fn prompt_renders_from_the_catalog() {
        let text = render(&SessionMessage::Prompt, "", &Locale::EN);
        assert!(text.contains("login"), "unexpected prompt: {text}");
    }

    #[test]
    fn puppet_list_names_every_character() {
        let names = vec![
            PuppetName::parse("arden").expect("name"),
            PuppetName::parse("borel").expect("name"),
        ];
        let text = render(&SessionMessage::PuppetList(names), "", &Locale::EN);
        assert!(text.contains("arden") && text.contains("borel"), "got: {text}");
    }
}
