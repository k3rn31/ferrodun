//! Multi-tenant boot: assembles one isolated stack per configured tenant —
//! its own DB, arena, scheduler, session registry, and TCP listener — bridged
//! to an embedded gateway over an in-memory IPC channel (design §Boot).

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use mud_db::{PersistentWorld, PlaceMap, TenantDb};
use mud_engine::{Dispatcher, Pipeline};
use mud_gateway::GatewayConfig;
use mud_ipc::in_memory_pair;
use mud_net::{DEFAULT_TENANT_TIER, resolve_tier};
use mud_world::{TenantConfig, load_world};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::task::JoinSet;
use tracing::Instrument;

use crate::backend::DbBackend;
use crate::config::ServerConfig;
use crate::places::WorldPlaces;
use crate::world_loop;

/// Boots every configured tenant and returns the bound addresses (in registry
/// order) plus the running task set. Ephemeral ports (":0") resolve to real
/// ones in the returned addresses — the integration-test seam.
///
/// # Errors
///
/// Returns an error if any tenant's config, world, or database fails to load,
/// or if its listener fails to bind.
pub async fn boot(
    config: ServerConfig,
) -> anyhow::Result<(Vec<SocketAddr>, JoinSet<anyhow::Result<()>>)> {
    let mut addrs = Vec::with_capacity(config.tenants.len());
    let mut tasks = JoinSet::new();

    for entry in &config.tenants {
        let tenant_config = TenantConfig::load(&entry.dir)
            .with_context(|| format!("loading tenant config at {}", entry.dir.display()))?;
        let loaded = load_world(&tenant_config)
            .with_context(|| format!("loading tenant world at {}", entry.dir.display()))?;

        let db = TenantDb::open(&entry.dir)
            .await
            .with_context(|| format!("opening tenant db at {}", entry.dir.display()))?;
        let world_id = db
            .world_id()
            .await
            .with_context(|| format!("reading world id at {}", entry.dir.display()))?;

        let place_map = PlaceMap::from_pairs(loaded.rooms().place_keys());
        let world = PersistentWorld::load(db.clone(), entry.tag, place_map)
            .await
            .with_context(|| format!("loading persistent world at {}", entry.dir.display()))?;
        let world = Arc::new(Mutex::new(world));

        let start_room = mud_core::PlaceKey::parse(tenant_config.start_room())
            .with_context(|| format!("parsing start room at {}", entry.dir.display()))?;
        let backend = DbBackend::new(db, world.clone(), start_room);

        let sessions = mud_engine::SessionService::new(loaded.banner(), tenant_config.locale());

        let mut dispatcher = Dispatcher::new();
        let builtins = mud_engine::register(&mut dispatcher);
        let pipeline = Pipeline::new(dispatcher).with_locale(tenant_config.locale());

        let listener = TcpListener::bind(entry.listen)
            .await
            .with_context(|| format!("binding tenant listener on {}", entry.listen))?;
        let bound_addr = listener
            .local_addr()
            .with_context(|| format!("reading bound address for {}", entry.listen))?;

        let (gateway_end, world_end) = in_memory_pair();
        let gateway_config = GatewayConfig {
            world_id,
            rate: config.rate,
            burst: config.burst,
            palette: Arc::new(loaded.palette().clone()),
            tier: resolve_tier(false, DEFAULT_TENANT_TIER),
        };
        // One span per tenant wraps both tasks: every event below — tick
        // events, dispatch warnings, i18n misses — inherits tenant identity
        // ambiently (design §4; SPEC §3.11.2).
        let tenant_span = tracing::info_span!(
            "tenant",
            tenant = entry.tag.get(),
            world_id = %world_id,
        );
        tasks.spawn({
            let span = tenant_span.clone();
            async move {
                mud_gateway::serve(listener, gateway_end, gateway_config)
                    .await
                    .map_err(anyhow::Error::from)
            }
            .instrument(span)
        });

        let places = WorldPlaces::new(loaded.rooms().clone());
        let runtime = world_loop::TenantRuntime {
            world,
            backend,
            sessions,
            pipeline,
            builtins,
            places,
            locale: tenant_config.locale(),
        };
        tasks.spawn(world_loop::run(world_end, world_id, runtime).instrument(tenant_span));

        addrs.push(bound_addr);
    }

    Ok((addrs, tasks))
}
