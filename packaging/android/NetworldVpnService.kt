package com.networld.vpn

import android.content.Intent
import android.net.VpnService
import android.os.ParcelFileDescriptor
import android.util.Log

/**
 * The phone-wide VPN. Establishes the TUN device, then hands its file
 * descriptor to Rust, which forwards every flow to Networld's local SOCKS5
 * port (the SSH / SSL / payload tunnel).
 */
class NetworldVpnService : VpnService() {
    companion object {
        const val ACTION_START = "com.networld.vpn.START"
        const val ACTION_STOP = "com.networld.vpn.STOP"
        const val EXTRA_PORT = "socksPort"
        private const val MTU = 1500
        private const val TAG = "NetworldVpn"
    }

    private var tun: ParcelFileDescriptor? = null
    private var worker: Thread? = null
    private val bridge = NativeBridge()

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP) {
            stopTunnel()
            stopSelf()
            return START_NOT_STICKY
        }
        startTunnel(intent?.getIntExtra(EXTRA_PORT, 1080) ?: 1080)
        return START_NOT_STICKY
    }

    @Synchronized
    private fun startTunnel(port: Int) {
        stopTunnel()
        val builder = Builder()
            .setSession("Networld VPN")
            .setMtu(MTU)
            .addAddress("10.255.0.2", 32)
            .addRoute("0.0.0.0", 0)
            .addDnsServer("10.255.0.1")
            // NOTE: leave the TUN fd NON-blocking (Android's default). The Rust
            // side reads it with epoll; a blocking fd stalls the whole tunnel.
        // IPv6 is routed into the tunnel and dropped there, so it cannot leak
        // around the VPN; apps fall back to IPv4.
        try {
            builder.addAddress("fd00:255::2", 128).addRoute("::", 0)
        } catch (e: Exception) {
            Log.w(TAG, "IPv6 route not added: $e")
        }
        try {
            // Our own sockets (the SSH link to the server) must bypass the VPN.
            builder.addDisallowedApplication(packageName)
        } catch (e: Exception) {
            Log.w(TAG, "could not exclude app: $e")
        }
        val fd = builder.establish()
        if (fd == null) {
            Log.e(TAG, "establish() returned null (permission revoked?)")
            stopSelf()
            return
        }
        tun = fd
        worker = Thread({
            val rc = bridge.run(fd.fd, port, MTU)
            Log.i(TAG, "tunnel ended rc=$rc")
            synchronized(this) {
                if (tun === fd) {
                    try { fd.close() } catch (_: Exception) {}
                    tun = null
                }
            }
        }, "networld-tun").also { it.start() }
    }

    @Synchronized
    private fun stopTunnel() {
        try { bridge.stop() } catch (_: Throwable) {}
        worker?.let { w ->
            try { w.join(3000) } catch (_: InterruptedException) {}
        }
        worker = null
        try { tun?.close() } catch (_: Exception) {}
        tun = null
    }

    override fun onRevoke() {
        stopTunnel()
        super.onRevoke()
    }

    override fun onDestroy() {
        stopTunnel()
        super.onDestroy()
    }
}
