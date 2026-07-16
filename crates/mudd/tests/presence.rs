//! Room-presence e2e (M1-27): two co-located telnet clients prove the
//! spawn/quit/socket-drop announcements the World loop fans out (§2.7 step 8).
#![allow(clippy::expect_used, clippy::panic)] // integration-test crates are not compiled with cfg(test), so clippy.toml allow-{expect,panic}-in-tests do not cover their helpers; both are permitted in tests per policy

use tempfile::TempDir;
use tokio::net::TcpStream;

mod common;

use common::{ClientReader, single_tenant_config, write_tenant};

/// Registers `username`, creates puppet `puppet`, and enters the world,
/// following the real login FSM's prompt sequence.
async fn register_and_enter(client: &mut ClientReader, username: &str, puppet: &str) {
    client.read_until(b"Welcome to Testville.").await;
    client.write_line(&format!("register {username}")).await;
    client.read_until(b"Password:").await;
    client.write_line("hunter2!").await;
    client.read_until(b"Confirm password:").await;
    client.write_line("hunter2!").await;
    client.read_until(b"You have no characters yet.").await;
    client.write_line(&format!("new {puppet}")).await;
    client
        .read_until(b"Welcome. You are now in the world.")
        .await;
}

/// Logs an existing `username` in and re-enters the world with `puppet`.
async fn login_and_play(client: &mut ClientReader, username: &str, puppet: &str) {
    client.read_until(b"Welcome to Testville.").await;
    client.write_line(&format!("login {username}")).await;
    client.read_until(b"Password:").await;
    client.write_line("hunter2!").await;
    client.read_until(puppet.as_bytes()).await;
    client.write_line(&format!("play {puppet}")).await;
    client
        .read_until(b"Welcome. You are now in the world.")
        .await;
}

#[tokio::test]
async fn presence_is_announced_and_listed() {
    let tenant_dir = TempDir::new().expect("temp dir");
    write_tenant(tenant_dir.path());
    let (addrs, _tasks) = mudd::boot(single_tenant_config(tenant_dir.path()))
        .await
        .expect("boot must succeed");
    let addr = *addrs.first().expect("one bound address");

    let stream = TcpStream::connect(addr).await.expect("alice connects");
    let mut alice = ClientReader::new(stream);
    register_and_enter(&mut alice, "alice", "Hero").await;

    // Spawn: Bob's arrival is announced to Alice, who shares the start room.
    let stream = TcpStream::connect(addr).await.expect("bob connects");
    let mut bob = ClientReader::new(stream);
    register_and_enter(&mut bob, "bob", "Sidekick").await;
    alice.read_until(b"Sidekick appears from nowhere.").await;

    // look lists the connected player.
    alice.write_line("look").await;
    alice.read_until(b"Sidekick is here.").await;

    // A clean quit is announced.
    bob.write_line("quit").await;
    alice.read_until(b"Sidekick disappears.").await;

    // After the leave, look no longer lists the body: bound the look reply
    // with a say echo and assert absence inside the captured bytes.
    alice.write_line("look").await;
    alice.write_line("say ping").await;
    let look_reply = alice.read_until(b"You say,").await;
    let needle = b"is here";
    assert!(
        !look_reply.windows(needle.len()).any(|w| w == needle),
        "a session-less puppet must not be listed, got {look_reply:?}"
    );

    // Reconnect, then hard-drop the socket: departure is announced again.
    let stream = TcpStream::connect(addr).await.expect("bob reconnects");
    let mut bob = ClientReader::new(stream);
    login_and_play(&mut bob, "bob", "Sidekick").await;
    alice.read_until(b"Sidekick appears from nowhere.").await;

    drop(bob);
    alice.read_until(b"Sidekick disappears.").await;
}
