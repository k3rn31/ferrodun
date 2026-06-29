//! CmdSet source layers and their fixed precedence (§2.7 step 4).
//!
//! The pipeline collects commands from several sources — account, puppet,
//! containers the caller is inside, the location, and channels — and merges them
//! into one [`CommandTable`]. When two sources contribute the same command name
//! under a `Union` merge, precedence resolves the collision in this fixed order,
//! highest first: **account → puppet → containers (innermost first) → location →
//! channels** (§2.7 step 4). That intent — the player's own bindings beat what
//! the world layers on, and channel commands never shadow a more local one — is
//! encoded by mapping each layer to a [`Priority`] and reusing
//! [`CmdSet::merge`].
//!
//! For M1 the account, container, and channel layers are present but empty:
//! accounts arrive in M1-18, channels in M3, and container traversal is
//! deferred. The puppet and location layers are exercised for real.

use mud_cmd::{CmdSet, CmdSetKey, Command, CommandTable, MergeType, Priority};

// The §2.7 step-4 precedence levels, highest first: distinct, descending values
// so a higher layer wins a `Union` collision. All containers share the single
// `CONTAINER` level; the innermost-first ordering *among* containers is
// deliberately not encoded as distinct per-depth priorities — that would impose
// an arbitrary nesting limit once the band ran out. Instead it falls out of push
// order, because `CmdSet::merge` resolves equal-priority `Union` ties in favour
// of the earlier set.
const ACCOUNT: i32 = 50;
const PUPPET: i32 = 40;
const CONTAINER: i32 = 30;
const LOCATION: i32 = 20;
const CHANNEL: i32 = 10;
// Engine built-ins (M1-17) sit below every other source so any account, puppet,
// container, location, or channel command of the same name shadows them — the
// player's world always beats the default binding (§2.7 step-4 intent).
const BUILTIN: i32 = 0;

/// The command sources for one resolved caller, one `Vec` per §2.7 step-4 layer.
///
/// `containers` is ordered innermost-first (the chest in your pack beats the
/// pack). For M1 `account`, `containers`, and `channels` stay empty; populating
/// them is M1-18 / future work and needs no change here.
#[derive(Debug, Default, Clone)]
#[must_use]
pub struct LayerCommands {
    /// The caller's account bindings (empty in M1; populated M1-18).
    pub account: Vec<Command>,
    /// The caller's puppet bindings.
    pub puppet: Vec<Command>,
    /// Bindings from containers the caller is inside, innermost first (empty in M1).
    pub containers: Vec<Vec<Command>>,
    /// Bindings contributed by the caller's current location.
    pub location: Vec<Command>,
    /// Bindings contributed by subscribed channels (empty in M1; §3.6.1).
    pub channels: Vec<Command>,
    /// Engine built-in commands (M1-17), at the lowest precedence so any other
    /// source shadows a same-named built-in.
    pub builtins: Vec<Command>,
}

impl LayerCommands {
    /// Merges every non-empty layer into one [`CommandTable`] under the §2.7
    /// step-4 precedence order.
    ///
    /// Each populated layer becomes a `Union` [`CmdSet`] at its mapped
    /// [`Priority`], and the merge resolves same-name collisions by that
    /// precedence. Every layer merges as `Union`: authoring explicit
    /// `Replace`/`Remove` overrides (§2.7 step 4) is deferred, so this type
    /// cannot yet express them — even though [`CmdSet::merge`] already honours
    /// them. Empty layers contribute nothing.
    pub fn merge(&self) -> CommandTable {
        let mut sets = Vec::new();

        push_layer(&mut sets, "account", ACCOUNT, &self.account);
        push_layer(&mut sets, "puppet", PUPPET, &self.puppet);
        // Innermost-first: containers are pushed most-local first, all at the
        // same `CONTAINER` level, so the equal-priority tie-break (earlier set
        // wins) makes an inner container beat an outer one at any nesting depth.
        for container in &self.containers {
            push_layer(&mut sets, "container", CONTAINER, container);
        }
        push_layer(&mut sets, "location", LOCATION, &self.location);
        push_layer(&mut sets, "channels", CHANNEL, &self.channels);
        push_layer(&mut sets, "builtins", BUILTIN, &self.builtins);

        CmdSet::merge(&sets)
    }
}

/// Appends a `Union` [`CmdSet`] for `commands` at `priority`, unless empty.
///
/// `key` names the layer for diagnostics only; collisions resolve by priority,
/// not by key. `key` is a `'static` layer name known to be a valid
/// [`CmdSetKey`], so parsing it cannot fail in practice — an unexpected failure
/// drops the layer rather than aborting the merge.
fn push_layer(sets: &mut Vec<CmdSet>, key: &str, level: i32, commands: &[Command]) {
    if commands.is_empty() {
        return;
    }
    let Ok(key) = CmdSetKey::parse(key) else {
        return;
    };
    sets.push(CmdSet::new(
        key,
        Priority::new(level),
        MergeType::Union,
        commands.to_vec(),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use mud_cmd::{CommandName, ParseOutcome};

    fn name(value: &str) -> CommandName {
        CommandName::parse(value).expect("valid command name")
    }

    /// A command answering to `canonical` plus one distinguishing alias, so the
    /// winning layer is identifiable after the merge.
    fn marked(canonical: &str, mark: &str) -> Command {
        Command::new(name(canonical)).with_alias(name(mark))
    }

    #[test]
    fn puppet_beats_location_on_a_collision() {
        let layers = LayerCommands {
            puppet: vec![marked("look", "from-puppet")],
            location: vec![marked("look", "from-location")],
            ..LayerCommands::default()
        };

        let table = layers.merge();
        let look = table.get(&name("look")).expect("look survives the merge");
        assert!(look.aliases().contains(&name("from-puppet")));
        assert!(!look.aliases().contains(&name("from-location")));
    }

    #[test]
    fn account_beats_puppet_on_a_collision() {
        let layers = LayerCommands {
            account: vec![marked("look", "from-account")],
            puppet: vec![marked("look", "from-puppet")],
            ..LayerCommands::default()
        };

        let table = layers.merge();
        let look = table.get(&name("look")).expect("look survives the merge");
        assert!(look.aliases().contains(&name("from-account")));
    }

    #[test]
    fn an_uncontested_location_command_is_present() {
        let layers = LayerCommands {
            location: vec![Command::new(name("exits"))],
            ..LayerCommands::default()
        };

        let table = layers.merge();
        assert!(table.get(&name("exits")).is_some());
    }

    #[test]
    fn a_channel_command_never_shadows_a_local_command() {
        let layers = LayerCommands {
            location: vec![marked("say", "from-location")],
            channels: vec![marked("say", "from-channel")],
            ..LayerCommands::default()
        };

        let table = layers.merge();
        let say = table.get(&name("say")).expect("say survives the merge");
        assert!(say.aliases().contains(&name("from-location")));
        assert!(!say.aliases().contains(&name("from-channel")));
    }

    #[test]
    fn an_inner_container_beats_an_outer_one() {
        let layers = LayerCommands {
            containers: vec![vec![marked("use", "inner")], vec![marked("use", "outer")]],
            ..LayerCommands::default()
        };

        let table = layers.merge();
        let used = table.get(&name("use")).expect("use survives the merge");
        assert!(used.aliases().contains(&name("inner")));
    }

    #[test]
    fn the_innermost_container_wins_at_any_nesting_depth() {
        // Precedence must not collapse once nesting outgrows any fixed band: the
        // innermost (first-pushed) container still wins, however deep the stack.
        let containers = (0..64)
            .map(|depth| vec![marked("use", &format!("c{depth}"))])
            .collect();
        let layers = LayerCommands {
            containers,
            ..LayerCommands::default()
        };

        let table = layers.merge();
        let used = table.get(&name("use")).expect("use survives the merge");
        assert!(used.aliases().contains(&name("c0")));
    }

    #[test]
    fn an_empty_layer_set_merges_to_an_empty_table() {
        let table = LayerCommands::default().merge();
        assert_eq!(table.parse("look"), ParseOutcome::NotFound);
    }

    #[test]
    fn a_location_command_shadows_a_same_named_builtin() {
        let layers = LayerCommands {
            location: vec![marked("look", "from-location")],
            builtins: vec![marked("look", "from-builtin")],
            ..LayerCommands::default()
        };

        let table = layers.merge();
        let look = table.get(&name("look")).expect("look survives the merge");
        assert!(look.aliases().contains(&name("from-location")));
        assert!(!look.aliases().contains(&name("from-builtin")));
    }

    #[test]
    fn an_uncontested_builtin_is_present() {
        let layers = LayerCommands {
            builtins: vec![Command::new(name("inventory"))],
            ..LayerCommands::default()
        };

        let table = layers.merge();
        assert!(table.get(&name("inventory")).is_some());
    }
}
