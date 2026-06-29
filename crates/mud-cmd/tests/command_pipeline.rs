//! The command pipeline end-to-end through the public surface: build layered
//! `CmdSet`s, `merge` them under §2.7-step-4 precedence, then `parse` lines
//! against the resulting `CommandTable` (§2.7 step 5). This mirrors what the
//! World pipeline (M1-16) will do once it maps the named source layers
//! (account → puppet → containers → location → channels) onto priorities.
#![allow(clippy::expect_used)] // test helpers; mirrors `allow-expect-in-tests`

use mud_cmd::{CmdSet, CmdSetKey, Command, CommandName, MergeType, ParseOutcome, Priority, Switch};

fn name(value: &str) -> CommandName {
    CommandName::parse(value).expect("valid command name")
}

fn switch(value: &str) -> Switch {
    Switch::parse(value).expect("valid switch")
}

/// A command answering to `canonical` plus the given aliases.
fn command(canonical: &str, aliases: &[&str]) -> Command {
    aliases
        .iter()
        .fold(Command::new(name(canonical)), |command, alias| {
            command.with_alias(name(alias))
        })
}

fn layer(key: &str, priority: i32, mergetype: MergeType, commands: Vec<Command>) -> CmdSet {
    CmdSet::new(
        CmdSetKey::parse(key).expect("valid key"),
        Priority::new(priority),
        mergetype,
        commands,
    )
}

/// A representative stack ordered like §2.7 step 4: account (highest) beats
/// puppet beats location beats channels. Each layer marks its commands with a
/// distinguishing alias so the winner is identifiable.
fn standard_stack() -> Vec<CmdSet> {
    vec![
        layer(
            "channels",
            10,
            MergeType::Union,
            vec![command("say", &["from-channel"])],
        ),
        layer(
            "location",
            20,
            MergeType::Union,
            vec![command("look", &["from-location"]), command("exits", &[])],
        ),
        layer(
            "puppet",
            30,
            MergeType::Union,
            vec![command("say", &["from-puppet"])],
        ),
        layer(
            "account",
            40,
            MergeType::Union,
            vec![
                command("look", &["l", "from-account"]),
                command("quit", &["q"]),
            ],
        ),
    ]
}

#[test]
fn the_account_binding_wins_a_collision_and_parses_to_that_command() {
    let table = CmdSet::merge(&standard_stack());

    let look = table.get(&name("look")).expect("look survives the merge");
    assert!(look.aliases().contains(&name("from-account")));
    assert!(!look.aliases().contains(&name("from-location")));

    // The same winning command is what parsing the canonical name resolves to.
    assert_eq!(
        table.parse("look"),
        ParseOutcome::Matched {
            command: look,
            switches: vec![],
            args: "",
        }
    );
}

#[test]
fn the_puppet_binding_beats_the_lower_channel_binding() {
    let table = CmdSet::merge(&standard_stack());

    let say = table.get(&name("say")).expect("say survives the merge");
    assert!(say.aliases().contains(&name("from-puppet")));
    assert!(!say.aliases().contains(&name("from-channel")));
}

#[test]
fn a_unique_prefix_and_an_exact_alias_both_resolve() {
    let table = CmdSet::merge(&standard_stack());

    let quit = table.get(&name("quit")).expect("quit survives the merge");
    assert_eq!(
        table.parse("qu"),
        ParseOutcome::Matched {
            command: quit,
            switches: vec![],
            args: "",
        }
    );

    let look = table.get(&name("look")).expect("look survives the merge");
    assert_eq!(
        table.parse("l"),
        ParseOutcome::Matched {
            command: look,
            switches: vec![],
            args: "",
        }
    );
}

#[test]
fn switches_and_arguments_survive_to_the_winning_command() {
    let table = CmdSet::merge(&standard_stack());
    let look = table.get(&name("look")).expect("look survives the merge");

    assert_eq!(
        table.parse("look/quiet north"),
        ParseOutcome::Matched {
            command: look,
            switches: vec![switch("quiet")],
            args: "north",
        }
    );
}

#[test]
fn a_low_priority_replace_overrides_a_higher_union_end_to_end() {
    // §2.7 step 4: Replace is an explicit override that wins regardless of
    // precedence — here from a set priced below the Union it overrides.
    let table = CmdSet::merge(&[
        layer(
            "account",
            100,
            MergeType::Union,
            vec![command("look", &["from-account"])],
        ),
        layer(
            "location",
            1,
            MergeType::Replace,
            vec![command("look", &["from-location"])],
        ),
    ]);

    let look = table.get(&name("look")).expect("look survives the merge");
    assert!(look.aliases().contains(&name("from-location")));
    assert_eq!(
        table.parse("look"),
        ParseOutcome::Matched {
            command: look,
            switches: vec![],
            args: "",
        }
    );
}

#[test]
fn a_remove_layer_deletes_the_command_and_parsing_reports_not_found() {
    let table = CmdSet::merge(&[
        layer(
            "account",
            100,
            MergeType::Union,
            vec![command("look", &[]), command("quit", &[])],
        ),
        layer("location", 1, MergeType::Remove, vec![command("quit", &[])]),
    ]);

    assert!(table.get(&name("quit")).is_none());
    assert_eq!(table.parse("quit"), ParseOutcome::NotFound);
    assert!(table.get(&name("look")).is_some());
}
