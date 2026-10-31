//! Android VPN bridge: reads packets from the VpnService TUN fd and forwards
//! every TCP/DNS flow into Networld's local SOCKS5 proxy (tun2proxy).

use networld_core::LogBus;
use std::sync::{Arc, Mutex, OnceLock};
use tun2proxy::{ArgDns, Args, CancellationToken, ProxyParameters};

static LOGS: OnceLock<Arc<LogBus>> = OnceLock::new();

/// Forwards tun2proxy's `log` output (warnings and errors) to the Logs tab.
struct BridgeLogger;

impl log::Log for BridgeLogger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Warn
    }
    fn log(&self, r: &log::Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        if let Some(l) = LOGS.get() {
            let msg = format!("{}", r.args());
            if r.level() == log::Level::Error {
                l.error("vpn-tun", msg);
            } else {
                l.warn("vpn-tun", msg);
            }
        }
    }
    fn flush(&self) {}
}

/// Call once, before `run`, so the bridge can report into the Logs tab.
pub fn init_logs(logs: Arc<LogBus>) {
    if LOGS.set(logs).is_ok() {
        let _ = log::set_boxed_logger(Box::new(BridgeLogger));
        log::set_max_level(log::LevelFilter::Warn);
    }
}

fn note(msg: String) {
    if let Some(l) = LOGS.get() {
        l.info("vpn-tun", msg);
    }
}

/// Where the server's UDP gateway (badvpn-udpgw) listens, as seen from the server.
const UDPGW_ADDR: [u8; 4] = [127, 0, 0, 1];
const UDPGW_PORT: u16 = 7300;

/// Asks our local SOCKS5 proxy to open 127.0.0.1:7300 on the server side. If it
/// answers "succeeded" the server runs a UDP gateway and UDP (calls, games,
/// QUIC) can ride the tunnel.
fn udpgw_reachable(socks_port: u16) -> bool {
    use std::io::{Read, Write};
    use std::time::Duration;
    let Ok(mut s) = std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], socks_port)),
        Duration::from_secs(2),
    ) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = s.set_write_timeout(Some(Duration::from_secs(5)));
    let mut hello = [0u8; 2];
    let mut req = vec![5, 1, 0, 5, 1, 0, 1];
    req.extend_from_slice(&UDPGW_ADDR);
    req.extend_from_slice(&UDPGW_PORT.to_be_bytes());
    if s.write_all(&req).is_err() || s.read_exact(&mut hello).is_err() {
        return false;
    }
    let mut reply = [0u8; 4];
    s.read_exact(&mut reply).is_ok() && reply[1] == 0
}

static QUIT: Mutex<Option<CancellationToken>> = Mutex::new(None);

/// Blocks until `stop()` is called or the tunnel dies. Returns 0 on a clean
/// stop, a negative number on failure.
pub fn run(tun_fd: i32, socks_port: u16, mtu: u16) -> i32 {
    note(format!(
        "TUN bridge starting (fd {tun_fd}, SOCKS5 127.0.0.1:{socks_port}, mtu {mtu})"
    ));
    let token = CancellationToken::new();
    match QUIT.lock() {
        Ok(mut q) => {
            if q.is_some() {
                return -1;
            }
            *q = Some(token.clone());
        }
        Err(_) => return -2,
    }

    let result = (|| -> Result<(), String> {
        let proxy = ProxyParameters::try_from(format!("socks5://127.0.0.1:{socks_port}"))
            .map_err(|e| format!("bad proxy address: {e}"))?;
        let mut args = Args::default();
        if udpgw_reachable(socks_port) {
            args.udpgw_server(std::net::SocketAddr::from((UDPGW_ADDR, UDPGW_PORT)));
            note("UDPGW found on the server: UDP traffic is tunnelled".into());
        } else if let Some(l) = LOGS.get() {
            l.warn(
                "vpn-tun",
                "This server does not support UDP (no UDPGW on 127.0.0.1:7300). \
                 Web and apps work, but calls, games and QUIC will not.",
            );
        }
        args.proxy(proxy)
            .tun_fd(Some(tun_fd))
            .close_fd_on_drop(false)
            .dns(ArgDns::Virtual)
            .ipv6_enabled(false)
            .setup(false)
            .verbosity(log::LevelFilter::Warn);
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|e| format!("runtime: {e}"))?;
        rt.block_on(tun2proxy::general_run_async(args, mtu, false, token))
            .map(|_| ())
            .map_err(|e| format!("tun2proxy: {e}"))
    })();

    if let Ok(mut q) = QUIT.lock() {
        *q = None;
    }
    match result {
        Ok(()) => {
            note("TUN bridge stopped".into());
            0
        }
        Err(e) => {
            log::error!("TUN bridge failed: {e}");
            -3
        }
    }
}

pub fn stop() -> i32 {
    match QUIT.lock() {
        Ok(mut q) => match q.take() {
            Some(t) => {
                t.cancel();
                0
            }
            None => -1,
        },
        Err(_) => -2,
    }
}
