//! Managed child processes for the pluggable engine slots
//! (xray, tun2socks, dnstt-client, badvpn-udpgw, wireguard).
//!
//! Every sidecar's stdout/stderr is piped straight into the app log bus, and
//! on Windows the console window is suppressed.

use crate::error::{CoreError, Result};
use crate::logging::LogBus;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration, Instant};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub struct Sidecar {
    pub name: String,
    pub bin: PathBuf,
    child: Mutex<Option<Child>>,
    logs: Arc<LogBus>,
}

impl Sidecar {
    /// Spawn `bin args…`, piping output to the log bus under `tag = name`.
    pub async fn spawn(
        name: &str,
        bin: PathBuf,
        args: Vec<String>,
        logs: Arc<LogBus>,
    ) -> Result<Arc<Self>> {
        if !bin.exists() {
            return Err(CoreError::MissingBinary(bin.display().to_string()));
        }
        logs.info(
            name,
            format!("Launching {} {}", bin.display(), args.join(" ")),
        );

        let mut cmd = Command::new(&bin);
        cmd.args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| CoreError::Other(format!("could not start {name}: {e}")))?;

        if let Some(out) = child.stdout.take() {
            let logs = logs.clone();
            let tag = name.to_string();
            tokio::spawn(async move {
                let mut lines = BufReader::new(out).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    logs.debug(&tag, line);
                }
            });
        }
        if let Some(err) = child.stderr.take() {
            let logs = logs.clone();
            let tag = name.to_string();
            tokio::spawn(async move {
                let mut lines = BufReader::new(err).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    logs.warn(&tag, line);
                }
            });
        }

        Ok(Arc::new(Self {
            name: name.to_string(),
            bin,
            child: Mutex::new(Some(child)),
            logs,
        }))
    }

    pub async fn is_running(&self) -> bool {
        let mut guard = self.child.lock().await;
        match guard.as_mut() {
            Some(c) => matches!(c.try_wait(), Ok(None)),
            None => false,
        }
    }

    pub async fn kill(&self) {
        let mut guard = self.child.lock().await;
        if let Some(child) = guard.as_mut() {
            let _ = child.start_kill();
            let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
            self.logs.info(&self.name, "Sidecar stopped");
        }
        *guard = None;
    }
}

/// Poll until something accepts TCP on `addr`, or give up.
pub async fn wait_for_port(addr: SocketAddr, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        if tokio::net::TcpStream::connect(addr).await.is_ok() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(CoreError::Timeout(timeout));
        }
        sleep(Duration::from_millis(120)).await;
    }
}

/// Find a free localhost TCP port (used for sidecar inbounds).
pub async fn free_port() -> Result<u16> {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    Ok(l.local_addr()?.port())
}
