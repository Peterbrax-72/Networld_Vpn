package com.networld.vpn

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.PowerManager
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

        /** The running service, so the app can drive it without startService(). */
        @Volatile
        var current: NetworldVpnService? = null
    }

    override fun onCreate() {
        super.onCreate()
        current = this
    }

    fun restart(port: Int) = startTunnel(port)

    fun shutdown() {
        stopTunnel()
        stopSelf()
    }

    private var tun: ParcelFileDescriptor? = null
    private var worker: Thread? = null
    private val bridge = NativeBridge()

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP) {
            // From the notification's Stop button: also disconnect the app.
            try { bridge.disconnect() } catch (_: Throwable) {}
            stopTunnel()
            stopSelf()
            return START_NOT_STICKY
        }
        goForeground()
        startTunnel(intent?.getIntExtra(EXTRA_PORT, 1080) ?: 1080)
        return START_NOT_STICKY
    }

    private var wake: PowerManager.WakeLock? = null

    /** Persistent notification (with Stop) + wake lock, so Android keeps the tunnel alive. */
    private fun goForeground() {
        try {
            val nm = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
            if (Build.VERSION.SDK_INT >= 26) {
                nm.createNotificationChannel(
                    NotificationChannel("networld_vpn", "VPN status", NotificationManager.IMPORTANCE_LOW)
                )
            }
            val stop = PendingIntent.getService(
                this, 1,
                Intent(this, NetworldVpnService::class.java).setAction(ACTION_STOP),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
            )
            val open = PendingIntent.getActivity(
                this, 2,
                packageManager.getLaunchIntentForPackage(packageName),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
            )
            val b = if (Build.VERSION.SDK_INT >= 26) Notification.Builder(this, "networld_vpn")
                    else Notification.Builder(this)
            val n = b.setContentTitle("Networld VPN")
                .setContentText("Connected - tap to open")
                .setSmallIcon(android.R.drawable.ic_lock_lock)
                .setOngoing(true)
                .setContentIntent(open)
                .addAction(Notification.Action.Builder(null, "Stop", stop).build())
                .build()
            if (Build.VERSION.SDK_INT >= 34) {
                startForeground(1, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_SYSTEM_EXEMPTED)
            } else {
                startForeground(1, n)
            }
        } catch (e: Exception) {
            // Notifications blocked or refused: the VPN still works, just without the pin.
            Log.w(TAG, "foreground notification failed: $e")
        }
        try {
            if (wake == null) {
                val pm = getSystemService(Context.POWER_SERVICE) as PowerManager
                wake = pm.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "networld:vpn").apply {
                    setReferenceCounted(false)
                    acquire()
                }
            }
        } catch (e: Exception) {
            Log.w(TAG, "wake lock failed: $e")
        }
    }

    private fun releaseWake() {
        try { wake?.let { if (it.isHeld) it.release() } } catch (_: Exception) {}
        wake = null
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
            val ended = synchronized(this) {
                if (tun === fd) {
                    try { fd.close() } catch (_: Exception) {}
                    tun = null
                    true
                } else false
            }
            // The bridge ended on its own (or was cancelled): drop the VPN so
            // the phone is never left routed into a dead tunnel.
            if (ended) stopSelf()
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
        releaseWake()
        if (current === this) current = null
        stopTunnel()
        super.onDestroy()
    }
}
