//! Small, room-agnostic helpers over the `kdl` crate's node model.
//!
//! Every loader (`rooms`, `regions`, `palette`) reads positional string
//! arguments off KDL nodes; this is that shared primitive, kept out of any one
//! loader so none has to depend on another for it.

use kdl::{KdlNode, KdlValue};

/// The first positional string argument of `node` at `index`, if present.
///
/// Returns `None` when the index is past the last argument or the value at that
/// position is not a string.
pub(crate) fn arg(node: &KdlNode, index: usize) -> Option<&str> {
    node.get(index).and_then(KdlValue::as_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kdl::KdlDocument;

    fn first_node(text: &str) -> KdlNode {
        let document = KdlDocument::parse(text).expect("valid kdl");
        document.nodes().first().expect("at least one node").clone()
    }

    #[test]
    fn arg_reads_a_positional_string() {
        let node = first_node("room \"town\" \"extra\"");
        assert_eq!(arg(&node, 0), Some("town"));
        assert_eq!(arg(&node, 1), Some("extra"));
    }

    #[test]
    fn arg_is_none_past_the_last_argument() {
        let node = first_node("room \"town\"");
        assert_eq!(arg(&node, 5), None);
    }

    #[test]
    fn arg_is_none_for_a_non_string_value() {
        let node = first_node("room 42");
        assert_eq!(arg(&node, 0), None);
    }
}
