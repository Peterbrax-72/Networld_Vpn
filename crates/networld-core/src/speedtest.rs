//! In-tunnel speed test: latency, download and upload throughput measured
//! through whichever [`Dialer`] is currently active.

use crate::dialer::Dialer;
use crate::error::{CoreError, Result};
use crate::logging::LogBus;
use crate::transport::{BoxStream, TlsOptions};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeedTestOptions {
    /// Any URL that returns a large body (default: Cloudflare's test endpoint).
    pub download_url: String,
    pub upload_url: Option<String>,
    pub max_bytes: u64,
    pub max_secs: u64,
}

impl Default for SpeedTestOptions {
    fn default() -> Self {
        Self {
            download_url: "https://speed.cloudflare.com/__down?bytes=25000000".into(),
            upload_url: Some("https://speed.cloudflare.com/__up".into()),
            max_bytes: 25_000_000,
            max_secs: 12,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeedTestResult {
    pub latency_ms: u64,
    pub jitter_ms: u64,
    pub download_bps: u64,
    pub upload_bps: u64,
    pub bytes_down: u64,
    pub bytes_up: u64,
    pub duration_ms: u64,
    pub endpoint: String,
}

/// Three TCP handshakes through the tunnel → latency + jitter.
pub async fn measure_latency(
    dialer: &Arc<dyn Dialer>,
    host: &str,
    port: u16,
) -> Result<(u64, u64)> {
    let mut samples = Vec::new();
    for _ in 0..3 {
        let t = Instant::now();
        let s = dialer.dial(host, port).await?;
        samples.push(t.elapsed().as_millis() as u64);
        drop(s);
    }
    let avg = samples.iter().sum::<u64>() / samples.len() as u64;
    let jitter = samples.iter().map(|s| s.abs_diff(avg)).max().unwrap_or(0);
    Ok((avg, jitter))
}

pub async fn run(
    dialer: Arc<dyn Dialer>,
    opts: &SpeedTestOptions,
    logs: Arc<LogBus>,
) -> Result<SpeedTestResult> {
    let url = crate::uri::parse(&opts.download_url)
        .ok_or_else(|| CoreError::parse("speed test URL", "not an absolute URL"))?;
    let host = url.host.clone();
    let port = if url.port == 0 { 443 } else { url.port };
    let secure = url.is_secure();

    logs.info("speedtest", format!("Measuring latency to {host}:{port}"));
    let (latency_ms, jitter_ms) = measure_latency(&dialer, &host, port).await?;

    logs.info("speedtest", "Starting download test");
    let started = Instant::now();
    let mut stream = open(&dialer, &host, port, secure).await?;
    let path = url.request_target();
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: NetworldVPN/1.0\r\nAccept: */*\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(req.as_bytes()).await?;
    stream.flush().await?;

    let mut buf = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    let deadline = Instant::now() + Duration::from_secs(opts.max_secs);
    let dl_start = Instant::now();
    loop {
        if Instant::now() >= deadline || total >= opts.max_bytes {
            break;
        }
        match tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf)).await {
            Ok(Ok(0)) | Err(_) => break,
            Ok(Ok(n)) => total += n as u64,
            Ok(Err(_)) => break,
        }
    }
    let dl_secs = dl_start.elapsed().as_secs_f64().max(0.001);
    let download_bps = ((total as f64) / dl_secs) as u64;

    // Upload: push zero-filled chunks for a few seconds.
    let mut bytes_up = 0u64;
    let mut upload_bps = 0u64;
    if let Some(up_url) = opts.upload_url.as_ref() {
        if let Some(u) = crate::uri::parse(up_url) {
            let uh = u.host.clone();
            let up_port = if u.port == 0 { 443 } else { u.port };
            let up_secure = u.is_secure();
            logs.info("speedtest", "Starting upload test");
            if let Ok(mut s) = open(&dialer, &uh, up_port, up_secure).await {
                let body_len = 8 * 1024 * 1024usize;
                let head = format!(
                    "POST {} HTTP/1.1\r\nHost: {uh}\r\nContent-Type: application/octet-stream\r\nContent-Length: {body_len}\r\nConnection: close\r\n\r\n",
                    u.request_target()
                );
                let _ = s.write_all(head.as_bytes()).await;
                let chunk = vec![0u8; 64 * 1024];
                let up_start = Instant::now();
                let up_deadline = up_start + Duration::from_secs(opts.max_secs.min(8));
                while bytes_up < body_len as u64 && Instant::now() < up_deadline {
                    match tokio::time::timeout(Duration::from_secs(5), s.write_all(&chunk)).await {
                        Ok(Ok(_)) => bytes_up += chunk.len() as u64,
                        Ok(Err(e)) => {
                            logs.warn("speedtest", format!("Upload write failed: {e}"));
                            break;
                        }
                        Err(_) => {
                            logs.warn(
                                "speedtest",
                                "Upload stalled: the tunnel accepted no data for 5s",
                            );
                            break;
                        }
                    }
                }
                let up_secs = up_start.elapsed().as_secs_f64().max(0.001);
                upload_bps = ((bytes_up as f64) / up_secs) as u64;
            }
        }
    }

    let result = SpeedTestResult {
        latency_ms,
        jitter_ms,
        download_bps,
        upload_bps,
        bytes_down: total,
        bytes_up,
        duration_ms: started.elapsed().as_millis() as u64,
        endpoint: format!("{host}:{port}"),
    };
    logs.info(
        "speedtest",
        format!(
            "Done — {} ms, ↓{} ↑{}",
            result.latency_ms,
            crate::stats::human_bytes(result.download_bps),
            crate::stats::human_bytes(result.upload_bps)
        ),
    );
    Ok(result)
}

async fn open(dialer: &Arc<dyn Dialer>, host: &str, port: u16, secure: bool) -> Result<BoxStream> {
    let raw = dialer.dial(host, port).await?;
    if secure {
        crate::tls::wrap(
            raw,
            host,
            &TlsOptions {
                sni: Some(host.to_string()),
                insecure: false,
                alpn: vec!["http/1.1".into()],
            },
            Duration::from_secs(15),
        )
        .await
    } else {
        Ok(raw)
    }
}
