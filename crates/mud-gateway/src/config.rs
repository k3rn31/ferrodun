//! Gateway runtime configuration.

use std::sync::Arc;

use mud_core::Palette;
use mud_net::{Burst, SustainedRate, Tier};
use mud_schema::WorldId;

/// Configuration for one [`serve`](crate::serve) run.
///
/// Values come from tenant configuration; this crate does no config loading
/// (that is `mudd`'s job, M1-22).
#[derive(Debug, Clone)]
#[must_use]
pub struct GatewayConfig {
    /// The World this gateway's IPC channel addresses (§2.1.3.2).
    pub world_id: WorldId,
    /// Per-session sustained command rate (§2.1.1; default 10/s).
    pub rate: SustainedRate,
    /// Per-session command burst allowance (§2.1.1; default 20).
    pub burst: Burst,
    /// The tenant palette session roles resolve against at render time
    /// (§3.20.3); shared read-only across connections.
    pub palette: Arc<Palette>,
    /// The color tier every session renders at. Fixed per gateway until
    /// per-session terminal negotiation lands (§3.20.5.2 step 3, M3).
    pub tier: Tier,
}
