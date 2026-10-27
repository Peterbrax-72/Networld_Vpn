//! SlowDNS / dnstt relay slot.
//!
//! Spawns `dnstt-client` so that SSH can ride a DNS tunnel:
//! `app → 127.0.0.1:listen_port → dnstt-client → resolver → NS domain → server`.

use crate::config::{Paths, SlowDnsProfile};
use crate::error::{CoreError, Result};
use crate::logging::LogBus;
use crate::sidecar::{wait_for_port, Sidecar};
use std::sync::Arc;
use std::time::Duration;

pub const BINARY: &str = "dnstt-client";

pub async fn start(
    profile: &SlowDnsProfile,
    paths: &Arc<Paths>,
    logs: Arc<LogBus>,
) -> Result<Arc<Sidecar>> {
    let bin = paths.binary(BINARY)?;
    let listen = format!("127.0.0.1:{}", profile.listen_port);

    let mut args: Vec<String> = Vec::new();
    match profile.transport.to_ascii_lowercase().as_str() {
        "doh" => {
            args.push("-doh".into());
            args.push(profile.resolver.clone());
        }
        "dot" => {
            args.push("-dot".into());
            args.push(profile.resolver.clone());
        }
        _ => {
            args.push("-udp".into());
            args.push(profile.resolver.clone());
        }
    }
    args.push("-pubkey".into());
    args.push(profile.public_key.clone());
    args.push(profile.domain.clone());
    args.push(listen.clone());

    logs.info(
        "slowdns",
        format!(
            "Starting DNS tunnel via {} ({}), NS {}",
            profile.resolver, profile.transport, profile.domain
        ),
    );
    let sc = Sidecar::spawn(BINARY, bin, args, logs.clone()).await?;

    let addr = listen
        .parse()
        .map_err(|_| CoreError::config(format!("bad SlowDNS listen address {listen}")))?;
    wait_for_port(addr, Duration::from_secs(20))
        .await
        .map_err(|_| CoreError::EngineUnavailable {
            engine: "slowdns".into(),
            reason: "the dnstt relay never opened its local port".into(),
        })?;
    logs.info("slowdns", format!("DNS relay ready on {listen}"));
    Ok(sc)
}
