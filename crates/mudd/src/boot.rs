//! Multi-tenant boot: assembles one isolated stack per configured tenant —
//! its own DB, arena, scheduler, session registry, and TCP listener — bridged
//! to an embedded gateway over an in-memory IPC channel (design §Boot).

use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use mud_db::{PersistentWorld, PlaceMap, TenantDb};
use mud_engine::{Dispatcher, Pipeline};
use mud_gateway::GatewayConfig;
use mud_ipc::in_memory_pair;
use mud_world::{TenantConfig, load_world};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::task::JoinSet;

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
/// if its listener fails to bind, or if two tenants share a `tenant_tag`.
pub async fn boot(
    config: ServerConfig,
) -> anyhow::Result<(Vec<SocketAddr>, JoinSet<anyhow::Result<()>>)> {
    let mut addrs = Vec::with_capacity(config.tenants.len());
    let mut tenant_tags = Vec::with_capacity(config.tenants.len());
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
        let world = PersistentWorld::load(db.clone(), tenant_config.tenant_tag(), place_map)
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
        };
        tasks.spawn(async move {
            mud_gateway::serve(listener, gateway_end, gateway_config)
                .await
                .map_err(anyhow::Error::from)
        });

        let places = WorldPlaces::new(loaded.rooms().clone());
        let runtime = world_loop::TenantRuntime {
            world,
            backend,
            sessions,
            pipeline,
            builtins,
            places,
        };
        tasks.spawn(world_loop::run(world_end, world_id, runtime));

        addrs.push(bound_addr);
        tenant_tags.push((entry.dir.clone(), tenant_config.tenant_tag()));
    }

    let mut seen = HashSet::with_capacity(tenant_tags.len());
    for (dir, tag) in &tenant_tags {
        if !seen.insert(tag) {
            anyhow::bail!(
                "duplicate tenant_tag across tenants: {} collides with an earlier tenant",
                dir.display()
            );
        }
    }

    Ok((addrs, tasks))
}
