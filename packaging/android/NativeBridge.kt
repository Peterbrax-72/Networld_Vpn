package com.networld.vpn

/** JNI bridge to the Rust tun2proxy runner (libnetworld_vpn_lib.so). */
class NativeBridge {
    companion object {
        init {
            System.loadLibrary("networld_vpn_lib")
        }
    }

    /** Blocks until stop() is called. 0 = clean stop, negative = failure. */
    external fun run(tunFd: Int, socksPort: Int, mtu: Int): Int

    external fun stop(): Int

    /** Disconnects the whole app (used by the notification's Stop button). */
    external fun disconnect(): Int
}
