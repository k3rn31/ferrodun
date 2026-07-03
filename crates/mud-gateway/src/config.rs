//! Gateway runtime configuration.

use mud_net::{Burst, SustainedRate};
use mud_schema::WorldId;

/// Configuration for one [`serve`](crate::serve) run.
///
/// Values come from tenant configuration; this crate does no config loading
/// (that is `mudd`'s job, M1-22).
#[derive(Debug, Clone, Copy)]
#[must_use]
pub struct GatewayConfig {
    /// The World this gateway's IPC channel addresses (§2.1.3.2).
    pub world_id: WorldId,
    /// Per-session sustained command rate (§2.1.1; default 10/s).
    pub rate: SustainedRate,
    /// Per-session command burst allowance (§2.1.1; default 20).
    pub burst: Burst,
}
