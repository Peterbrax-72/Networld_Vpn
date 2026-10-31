package com.networld.vpn

import android.app.Activity
import android.content.Intent
import android.net.VpnService
import androidx.activity.result.ActivityResult
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin

@InvokeArg
class StartArgs {
    var socksPort: Int = 1080
}

/** Called from Rust (android_vpn.rs) to start / stop the device-wide VPN. */
@TauriPlugin
class VpnPlugin(private val activity: Activity) : Plugin(activity) {
    private var pendingPort = 1080

    @Command
    fun startVpn(invoke: Invoke) {
        pendingPort = invoke.parseArgs(StartArgs::class.java).socksPort
        // Android 13+: ask once so the status notification can show.
        if (android.os.Build.VERSION.SDK_INT >= 33 &&
            activity.checkSelfPermission("android.permission.POST_NOTIFICATIONS") !=
                android.content.pm.PackageManager.PERMISSION_GRANTED
        ) {
            try {
                activity.requestPermissions(arrayOf("android.permission.POST_NOTIFICATIONS"), 4242)
            } catch (_: Exception) {}
        }
        val consent = VpnService.prepare(activity)
        val running = NetworldVpnService.current
        if (consent == null && running != null) {
            // Already running (e.g. the local port changed after a reconnect):
            // no startService() needed, which also works from the background.
            running.restart(pendingPort)
            invoke.resolve()
        } else if (consent != null) {
            // First run: Android shows its "Connection request" dialog.
            startActivityForResult(invoke, consent, "onVpnConsent")
        } else {
            launch()
            invoke.resolve()
        }
    }

    @ActivityCallback
    fun onVpnConsent(invoke: Invoke, result: ActivityResult) {
        if (result.resultCode == Activity.RESULT_OK) {
            launch()
            invoke.resolve()
        } else {
            invoke.reject("VPN permission was denied")
        }
    }

    @Command
    fun stopVpn(invoke: Invoke) {
        val running = NetworldVpnService.current
        if (running != null) {
            running.shutdown()
        } else {
            try {
                activity.startService(
                    Intent(activity, NetworldVpnService::class.java).setAction(NetworldVpnService.ACTION_STOP)
                )
            } catch (_: Exception) {}
        }
        invoke.resolve()
    }

    private fun launch() {
        activity.startService(
            Intent(activity, NetworldVpnService::class.java)
                .setAction(NetworldVpnService.ACTION_START)
                .putExtra(NetworldVpnService.EXTRA_PORT, pendingPort)
        )
    }
}
