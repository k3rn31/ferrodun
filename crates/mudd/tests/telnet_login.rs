//! The M1-22 Definition of Done: a real telnet client driven end-to-end
//! through `mudd`'s boot → gateway → World loop → session FSM → command
//! pipeline, plus a concurrent two-tenant boot proving per-tenant isolation.
#![allow(clippy::expect_used, clippy::panic)] // integration-test crates are not compiled with cfg(test), so clippy.toml allow-{expect,panic}-in-tests do not cover their helpers; both are permitted in tests per policy

use std::path::Path;
use std::time::Duration;

use mud_core::TenantTag;
use mud_net::{Burst, SustainedRate};
use mudd::{LogFormat, ServerConfig, TenantEntry};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

const TICK: Duration = Duration::from_secs(5);

/// Writes a minimal, self-contained tenant directory: one region, one room
/// with no exits, and a welcome banner (no dangling references).
fn write_tenant(dir: &Path) {
    std::fs::write(dir.join("config.toml"), "start_room = \"town_square\"\n")
        .expect("write config.toml");
    std::fs::write(
        dir.join("welcome.kdl"),
        "banner \"Welcome to Testville.\"\n",
    )
    .expect("write welcome.kdl");

    let world = dir.join("world/town");
    std::fs::create_dir_all(&world).expect("create world dir");
    std::fs::write(
        world.join("region.kdl"),
        "region \"town\" {\n    name \"Town\"\n}\n",
    )
    .expect("write region.kdl");
    std::fs::write(
        world.join("town.kdl"),
        "room \"town_square\" {\n    title \"Town Square\"\n    description \"A test square.\"\n}\n",
    )
    .expect("write town.kdl");
}

/// Buffers telnet client bytes across calls so a needle satisfied by a later
/// call is not silently dropped when it arrives batched with an earlier one.
struct ClientReader {
    stream: TcpStream,
    pending: Vec<u8>,
}

impl ClientReader {
    fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            pending: Vec::new(),
        }
    }

    /// Reads until `needle` has appeared in the stream, leaving any bytes
    /// after the match buffered for the next call.
    async fn read_until(&mut self, needle: &[u8]) -> Vec<u8> {
        let mut buf = [0u8; 512];
        loop {
            if let Some(end) = self
                .pending
                .windows(needle.len())
                .position(|w| w == needle)
                .map(|start| start + needle.len())
            {
                let rest = self.pending.split_off(end);
                let matched = std::mem::replace(&mut self.pending, rest);
                return matched;
            }

            let n = timeout(TICK, self.stream.read(&mut buf))
                .await
                .expect("client read must not time out")
                .expect("client read must succeed");
            assert!(n > 0, "socket closed before expected bytes arrived");
            self.pending
                .extend_from_slice(buf.get(..n).expect("read length is within buffer"));
        }
    }

    async fn write_line(&mut self, line: &str) {
        let mut bytes = line.as_bytes().to_vec();
        bytes.extend_from_slice(b"\r\n");
        timeout(TICK, self.stream.write_all(&bytes))
            .await
            .expect("write must not time out")
            .expect("write must succeed");
    }
}

fn single_tenant_config(dir: &Path) -> ServerConfig {
    ServerConfig {
        rate: SustainedRate::DEFAULT,
        burst: Burst::DEFAULT,
        tenants: vec![TenantEntry {
            dir: dir.to_path_buf(),
            listen: "127.0.0.1:0".parse().expect("addr"),
            tag: TenantTag::new(1).expect("tag 1 is in range"),
        }],
        log_format: LogFormat::default(),
    }
}

/// Drives one telnet session from a fresh connection all the way to a
/// working in-world `look` command.
async fn login_and_enter_world(client: &mut ClientReader) {
    client.read_until(b"Welcome to Testville.").await;

    client.write_line("register alice").await;
    client.read_until(b"Password:").await;

    client.write_line("hunter2!").await;
    client.read_until(b"Confirm password:").await;

    client.write_line("hunter2!").await;
    client.read_until(b"You have no characters yet.").await;

    client.write_line("new Hero").await;
    client.read_until(b"Created Hero.").await;

    client
        .read_until(b"Welcome. You are now in the world.")
        .await;
}

#[tokio::test]
async fn a_full_register_create_enter_flow_over_telnet() {
    let tenant_dir = TempDir::new().expect("temp dir");
    write_tenant(tenant_dir.path());

    let (addrs, _tasks) = mudd::boot(single_tenant_config(tenant_dir.path()))
        .await
        .expect("boot must succeed");
    let addr = *addrs.first().expect("one bound address");

    let stream = TcpStream::connect(addr).await.expect("client must connect");
    let mut client = ClientReader::new(stream);
    login_and_enter_world(&mut client).await;

    client.write_line("look").await;
    client.read_until(b"Town Square").await;
}

#[tokio::test]
async fn two_tenants_serve_independent_logins_at_once() {
    let tenant_a = TempDir::new().expect("temp dir a");
    let tenant_b = TempDir::new().expect("temp dir b");
    write_tenant(tenant_a.path());
    write_tenant(tenant_b.path());

    let config = ServerConfig {
        rate: SustainedRate::DEFAULT,
        burst: Burst::DEFAULT,
        tenants: vec![
            TenantEntry {
                dir: tenant_a.path().to_path_buf(),
                listen: "127.0.0.1:0".parse().expect("addr"),
                tag: TenantTag::new(1).expect("tag 1 is in range"),
            },
            TenantEntry {
                dir: tenant_b.path().to_path_buf(),
                listen: "127.0.0.1:0".parse().expect("addr"),
                tag: TenantTag::new(2).expect("tag 2 is in range"),
            },
        ],
        log_format: LogFormat::default(),
    };

    let (addrs, _tasks) = mudd::boot(config).await.expect("boot must succeed");
    assert_eq!(addrs.len(), 2, "both tenants must bind a distinct address");

    let addr_a = *addrs.first().expect("tenant a bound an address");
    let addr_b = *addrs.get(1).expect("tenant b bound an address");
    let stream_a = TcpStream::connect(addr_a).await.expect("client a connects");
    let stream_b = TcpStream::connect(addr_b).await.expect("client b connects");
    let mut client_a = ClientReader::new(stream_a);
    let mut client_b = ClientReader::new(stream_b);

    // Same username on both tenants: per-tenant DB isolation means no
    // cross-tenant collision (SPEC §2.5.1.4).
    tokio::join!(
        login_and_enter_world(&mut client_a),
        login_and_enter_world(&mut client_b),
    );
}
