//! A server for `cargo xtask idle-bench`: starts with its relay, creates an
//! admin, an admin key and an invitation, prints the invitation link on
//! standard output, then runs until standard input closes.
//!
//! Development tool, not shipped.

use std::io::{BufRead, Write};

use anyhow::Context;
use banc_proto::invite::{Invite, InviteToken};
use banc_proto::keys::{Fingerprint, SecretKey};
use banc_proto::machine::{MachineRole, PersonRole};
use banc_server::config::Config;
use banc_server::{auth, server, state, store};

fn free_port() -> anyhow::Result<u16> {
    Ok(std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port())
}

fn main() -> anyhow::Result<()> {
    let dir = std::env::args()
        .nth(1)
        .context("usage: bench_server <data folder>")?;
    let (relay, iroh) = (free_port()?, free_port()?);
    let text = format!(
        "data_dir = '{dir}'\npublic_url = \"http://localhost:8080\"\n\
         [http]\nlisten = \"127.0.0.1:0\"\n\
         [iroh]\nlisten = \"127.0.0.1:{iroh}\"\npublic_addrs = [\"127.0.0.1:{iroh}\"]\n\
         [relay]\nlisten = \"127.0.0.1:{relay}\"\nurl = \"http://localhost:{relay}\"\n"
    );
    let config = Config::parse(&text, std::path::Path::new("."))?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let running = server::start(&config, state::system_clock()).await?;
        let now = running.state.now();
        let admin_key = SecretKey::generate().public();
        let token = InviteToken::from_bytes(auth::random_bytes()?);
        {
            let mut db = running.state.db();
            let admin = store::create_admin(&db, "bench", "-", now)?.context("admin")?;
            store::set_admin_key(&mut db, &admin_key, admin, now)?;
            let invite = store::NewInvite {
                token_hash: &auth::token_hash(token.as_bytes()),
                user_name: "bench",
                person_role: PersonRole::Editor,
                machine_role: MachineRole::Workstation,
                admin_key: &admin_key,
                created_by: admin,
            };
            store::create_invite(&db, &invite, now)?;
        }
        let invite = Invite {
            server: format!("http://{}", running.http_addr),
            token,
            fingerprint: Fingerprint::of(&admin_key),
        };
        let mut stdout = std::io::stdout();
        stdout.write_all(format!("{}\n", invite.link()).as_bytes())?;
        stdout.flush()?;
        // Runs until the caller closes standard input.
        tokio::task::spawn_blocking(|| {
            let mut line = String::new();
            while std::io::stdin().lock().read_line(&mut line).unwrap_or(0) > 0 {
                line.clear();
            }
        })
        .await?;
        running.shutdown().await
    })
}
