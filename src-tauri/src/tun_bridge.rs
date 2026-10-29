//! Android VPN bridge: reads packets from the VpnService TUN fd and forwards
//! every TCP/DNS flow into Networld's local SOCKS5 proxy (tun2proxy).

use std::sync::Mutex;
use tun2proxy::{ArgDns, Args, CancellationToken, ProxyParameters};

static QUIT: Mutex<Option<CancellationToken>> = Mutex::new(None);

/// Blocks until `stop()` is called or the tunnel dies. Returns 0 on a clean
/// stop, a negative number on failure.
pub fn run(tun_fd: i32, socks_port: u16, mtu: u16) -> i32 {
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
        args.proxy(proxy)
            .tun_fd(Some(tun_fd))
            .close_fd_on_drop(false)
            .dns(ArgDns::Virtual)
            .ipv6_enabled(false)
            .setup(false)
            .verbosity(log::LevelFilter::Info);
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
        Ok(()) => 0,
        Err(e) => {
            log::error!("{e}");
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
