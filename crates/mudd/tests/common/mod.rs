//! Shared telnet-integration harness: a tenant fixture writer, a buffering
//! client reader, and a single-tenant `ServerConfig` builder, reused by every
//! `mudd` integration test that drives a real socket.

use std::path::Path;
use std::time::Duration;

use mud_core::TenantTag;
use mud_net::{Burst, SustainedRate};
use mudd::{LogFormat, ServerConfig, TenantEntry};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

const TICK: Duration = Duration::from_secs(5);

/// Writes a minimal, self-contained tenant directory: one region, one room
/// with no exits, and a welcome banner (no dangling references).
pub fn write_tenant(dir: &Path) {
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
pub struct ClientReader {
    stream: TcpStream,
    pending: Vec<u8>,
}

impl ClientReader {
    pub fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            pending: Vec::new(),
        }
    }

    /// Reads until `needle` has appeared in the stream, leaving any bytes
    /// after the match buffered for the next call.
    pub async fn read_until(&mut self, needle: &[u8]) -> Vec<u8> {
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

    pub async fn write_line(&mut self, line: &str) {
        let mut bytes = line.as_bytes().to_vec();
        bytes.extend_from_slice(b"\r\n");
        timeout(TICK, self.stream.write_all(&bytes))
            .await
            .expect("write must not time out")
            .expect("write must succeed");
    }
}

pub fn single_tenant_config(dir: &Path) -> ServerConfig {
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
