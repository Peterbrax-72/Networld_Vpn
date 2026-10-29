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
        val consent = VpnService.prepare(activity)
        if (consent != null) {
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
        activity.startService(
            Intent(activity, NetworldVpnService::class.java).setAction(NetworldVpnService.ACTION_STOP)
        )
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
