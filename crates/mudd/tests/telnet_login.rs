//! The M1-22 Definition of Done: a real telnet client driven end-to-end
//! through `mudd`'s boot → gateway → World loop → session FSM → command
//! pipeline, plus a concurrent two-tenant boot proving per-tenant isolation.
#![allow(clippy::expect_used, clippy::panic)] // integration-test crates are not compiled with cfg(test), so clippy.toml allow-{expect,panic}-in-tests do not cover their helpers; both are permitted in tests per policy

use mud_core::TenantTag;
use mud_net::{Burst, SustainedRate};
use mudd::{LogFormat, ServerConfig, TenantEntry};
use tempfile::TempDir;
use tokio::net::TcpStream;

mod common;

use common::{ClientReader, single_tenant_config, write_tenant};

/// Drives one telnet session from a fresh connection all the way to a
/// working in-world `look` command.
async fn login_and_enter_world(client: &mut ClientReader) {
    client.read_until(b"Welcome to Testville.").await;

    client.write_line("register alice").await;
    let to_password = client.read_until(b"Password:").await;
    assert!(
        to_password.windows(3).any(|w| w == [255, 251, 1]),
        "IAC WILL ECHO must precede the password prompt, got {to_password:?}"
    );

    client.write_line("hunter2!").await;
    let to_confirm = client.read_until(b"Confirm password:").await;
    assert!(
        !to_confirm.windows(3).any(|w| w == [255, 252, 1]),
        "echo must stay suppressed across the confirm prompt, got {to_confirm:?}"
    );

    client.write_line("hunter2!").await;
    let after_secret = client.read_until(b"You have no characters yet.").await;
    assert!(
        after_secret.windows(3).any(|w| w == [255, 252, 1]),
        "IAC WONT ECHO must follow the final password line, got {after_secret:?}"
    );

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
    let look_reply = client.read_until(b"Town Square").await;
    // M1-26: the reply must carry ANSI escapes — the room title/exits are
    // styled, and the gateway now renders at ansi16.
    assert!(
        look_reply.windows(2).any(|w| w == b"\x1b["),
        "look reply must contain ANSI escapes, got {look_reply:?}"
    );
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

#[tokio::test]
async fn login_masks_the_password_like_registration() {
    let tenant_dir = TempDir::new().expect("temp dir");
    write_tenant(tenant_dir.path());

    let (addrs, _tasks) = mudd::boot(single_tenant_config(tenant_dir.path()))
        .await
        .expect("boot must succeed");
    let addr = *addrs.first().expect("one bound address");

    // First connection registers alice and creates the puppet Hero.
    let stream = TcpStream::connect(addr).await.expect("client must connect");
    let mut client = ClientReader::new(stream);
    login_and_enter_world(&mut client).await;
    client.write_line("quit").await;
    drop(client);

    // Second connection logs in; the password prompt must be masked too.
    let stream = TcpStream::connect(addr)
        .await
        .expect("client must reconnect");
    let mut client = ClientReader::new(stream);
    client.read_until(b"Welcome to Testville.").await;

    client.write_line("login alice").await;
    let to_password = client.read_until(b"Password:").await;
    assert!(
        to_password.windows(3).any(|w| w == [255, 251, 1]),
        "IAC WILL ECHO must precede the login password prompt, got {to_password:?}"
    );

    client.write_line("hunter2!").await;
    // The first post-auth output is the puppet list naming Hero; the echo
    // release must have been written by then.
    let after_secret = client.read_until(b"Hero").await;
    assert!(
        after_secret.windows(3).any(|w| w == [255, 252, 1]),
        "IAC WONT ECHO must follow the password line, got {after_secret:?}"
    );
}
