//! Android device-wide VPN.
//!
//! Flow: the core connects (SSH etc.) and exposes a local SOCKS5 port. When the
//! state becomes Connected we ask the Kotlin `VpnPlugin` to start Android's
//! VpnService; its TUN device is handed to `tun_bridge` (tun2proxy), which feeds
//! every TCP flow and DNS lookup of the phone into that SOCKS5 port.
//! Our own app is excluded from the VPN (addDisallowedApplication), so the SSH
//! connection to the server does not loop back into the tunnel.

use networld_core::engine::ConnectionState;
use networld_core::manager::StatusSnapshot;
use networld_core::VpnManager;
use serde_json::json;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
use tauri::{Manager, Runtime, Wry};

pub struct VpnHandle<R: Runtime>(pub PluginHandle<R>);

/// Port the tunnel currently forwards to (0 = VPN off).
static ACTIVE_PORT: AtomicU16 = AtomicU16::new(0);

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("networld-vpn")
        .setup(|app, api| {
            let handle = api.register_android_plugin("com.networld.vpn", "VpnPlugin")?;
            app.manage(VpnHandle(handle));
            Ok(())
        })
        .build()
}

/// Called for every status change; starts/stops the VpnService to match.
pub fn sync(app: &tauri::AppHandle, manager: &Arc<VpnManager>, st: &StatusSnapshot) {
    crate::tun_bridge::init_logs(manager.logs.clone());
    match st.state {
        ConnectionState::Connected => {
            let Some(port) = st.socks_port else { return };
            if ACTIVE_PORT.swap(port, Ordering::SeqCst) == port {
                return;
            }
            let app = app.clone();
            let manager = manager.clone();
            tauri::async_runtime::spawn(async move {
                let a = app.clone();
                let res = tauri::async_runtime::spawn_blocking(move || {
                    a.state::<VpnHandle<Wry>>()
                        .0
                        .run_mobile_plugin::<serde_json::Value>(
                            "startVpn",
                            json!({ "socksPort": port }),
                        )
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                })
                .await;
                match res {
                    Ok(Ok(())) => manager.logs.info("vpn", "Device-wide VPN is on"),
                    other => {
                        ACTIVE_PORT.store(0, Ordering::SeqCst);
                        manager.logs.error(
                            "vpn",
                            format!("Could not start the VPN: {other:?}. Disconnecting."),
                        );
                        let _ = manager.disconnect().await;
                    }
                }
            });
        }
        // Keep the tunnel up while reconnecting so traffic never leaks around it.
        ConnectionState::Disconnected | ConnectionState::Error => {
            if ACTIVE_PORT.swap(0, Ordering::SeqCst) != 0 {
                let app = app.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let _ = app
                        .state::<VpnHandle<Wry>>()
                        .0
                        .run_mobile_plugin::<serde_json::Value>("stopVpn", json!({}));
                });
            }
        }
        _ => {}
    }
}

// --- JNI entry points used by com.networld.vpn.NativeBridge (Kotlin) ---------

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_networld_vpn_NativeBridge_run(
    _env: *mut c_void,
    _this: *mut c_void,
    tun_fd: i32,
    socks_port: i32,
    mtu: i32,
) -> i32 {
    crate::tun_bridge::run(tun_fd, socks_port as u16, mtu as u16)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_networld_vpn_NativeBridge_stop(
    _env: *mut c_void,
    _this: *mut c_void,
) -> i32 {
    crate::tun_bridge::stop()
}
