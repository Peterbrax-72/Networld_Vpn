# Adding a tunnel engine

Three edits, one new file. The UI, the capability matrix, the Store gating and
the "engine binary missing" messaging all follow automatically.

## 1. The trait

```rust
// crates/networld-core/src/engine.rs
#[async_trait]
pub trait TunnelEngine: Send + Sync {
    fn kind(&self) -> EngineKind;
    fn capabilities(&self) -> EngineCapabilities;
    async fn start(&self, profile: &ServerProfile, ctx: EngineContext)
        -> Result<Box<dyn EngineHandle>, CoreError>;
}

#[async_trait]
pub trait EngineHandle: Send + Sync {
    fn dialer(&self) -> Arc<dyn Dialer>;   // how outbound TCP leaves the tunnel
    async fn health(&self) -> ConnectionState;
    async fn stop(&self);
}
```

`EngineCapabilities`:

| Field | Meaning |
| --- | --- |
| `udp` | engine can carry UDP (enables the UDPGW / TUN UDP paths) |
| `tun_mode` | engine can back a system-wide TUN adapter |
| `requires_elevation` | needs administrator rights |
| `store_safe` | may be offered in the Microsoft Store build |
| `sidecar` | file name expected in `resources/bin`, or `None` for native |

## 2. The file

```rust
// crates/networld-core/src/engines/myproto.rs
pub struct MyProtoEngine;

#[async_trait]
impl TunnelEngine for MyProtoEngine {
    fn kind(&self) -> EngineKind { EngineKind::MyProto }

    fn capabilities(&self) -> EngineCapabilities {
        EngineCapabilities {
            udp: true,
            tun_mode: false,
            requires_elevation: false,
            store_safe: true,
            sidecar: Some("myproto.exe"),
        }
    }

    async fn start(&self, profile: &ServerProfile, ctx: EngineContext)
        -> Result<Box<dyn EngineHandle>, CoreError> {
        // native: build a Dialer directly
        // sidecar: ctx.paths.binary("myproto.exe")? then sidecar::spawn(...)
        //          and return a Socks5Dialer aimed at its local port
    }
}
```

## 3. Wire it up

```rust
// crates/networld-core/src/engines/mod.rs
pub fn build(kind: EngineKind) -> Result<Box<dyn TunnelEngine>, CoreError> {
    Ok(match kind {
        …
        EngineKind::MyProto => Box::new(myproto::MyProtoEngine),
    })
}

pub fn capability_matrix() -> Vec<(EngineKind, EngineCapabilities)> {
    vec![ …, (EngineKind::MyProto, MyProtoEngine.capabilities()) ]
}
```

Add the variant to `EngineKind` (`engine.rs`), a `ProfileKind` variant plus its
settings struct (`config.rs`), and the matching TypeScript shape in
`src/types.ts` — then the server editor can render fields for it.

> ⚠️ A new `ProfileKind` struct is `#[serde(flatten)]`-ed into `ServerProfile`.
> Do not reuse the field names `id`, `name`, `group`, `notes`, `tags`,
> `favorite`, `locked`, `createdAtMs`. Extend the
> `flattened_kinds_do_not_collide_with_profile_id` test with your new kind.

## Sidecar engines

`sidecar.rs` handles spawn, `CREATE_NO_WINDOW`, stdout/stderr → `LogBus`,
readiness polling on the local port, and kill-on-drop. Put the executable in
`src-tauri/resources/bin/`; `Paths::binary()` resolves it next to the app
executable at runtime and returns a user-facing error when it is absent, which
surfaces as `AppInfo.engines[].reason` in Tools → Engines.

## Current matrix

| Engine | Sidecar | UDP | TUN | Elevation | Store |
| --- | --- | --- | --- | --- | --- |
| SSH (direct / TLS / proxy + payload) | — native | via UDPGW | ✅ via tun2socks | ✗ | ✅ |
| SlowDNS / DNSTT → SSH | `dnstt-client` | via UDPGW | ✗ | ✗ | ✅ |
| Xray (VMess/VLESS/Trojan/SS) | `xray` | ✅ | ✅ via tun2socks | ✗ | ✅ |
| WireGuard | `wireguard` | ✅ | ✅ (is a tunnel) | ✅ | ✗ |
