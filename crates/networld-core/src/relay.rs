//! Byte-counting bidirectional relay shared by the SOCKS5 and HTTP front-ends.

use crate::stats::TrafficCounters;
use crate::transport::BoxStream;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Per-direction copy buffer. 16 KiB keeps a busy tunnel (hundreds of
/// concurrent sockets) in a few megabytes of RSS without costing throughput.
const BUF: usize = 32 * 1024;

/// Pump `client` ⇄ `remote`, attributing bytes to the session counters.
/// Returns `(uploaded, downloaded)`.
pub async fn pipe(client: BoxStream, remote: BoxStream, stats: Arc<TrafficCounters>) -> (u64, u64) {
    let (mut cr, mut cw) = tokio::io::split(client);
    let (mut rr, mut rw) = tokio::io::split(remote);

    let up_stats = stats.clone();
    let up = tokio::spawn(async move {
        let mut buf = vec![0u8; BUF];
        let mut total = 0u64;
        loop {
            match cr.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if rw.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                    total += n as u64;
                    up_stats.add_up(n as u64);
                }
            }
        }
        let _ = rw.shutdown().await;
        total
    });

    let down_stats = stats.clone();
    let down = tokio::spawn(async move {
        let mut buf = vec![0u8; BUF];
        let mut total = 0u64;
        loop {
            match rr.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if cw.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                    total += n as u64;
                    down_stats.add_down(n as u64);
                }
            }
        }
        let _ = cw.shutdown().await;
        total
    });

    let (u, d) = tokio::join!(up, down);
    (u.unwrap_or(0), d.unwrap_or(0))
}
