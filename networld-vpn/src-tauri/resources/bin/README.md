# resources/bin — engine sidecars

Drop Windows executables here. They are copied next to `NetworldVPN.exe` at
install time and resolved at runtime by `networld_core::config::Paths::binary()`.

| File | Project | Powers | Licence |
| --- | --- | --- | --- |
| `xray.exe` | [XTLS/Xray-core](https://github.com/XTLS/Xray-core) | VMess, VLESS, Trojan, Shadowsocks | MPL-2.0 |
| `geoip.dat`, `geosite.dat` | Xray assets | routing rules | CC-BY-SA |
| `tun2socks.exe` | [xjasonlyu/tun2socks](https://github.com/xjasonlyu/tun2socks) | system-wide TUN mode | GPL-3.0 |
| `dnstt-client.exe` | [dnstt](https://www.bamsoftware.com/software/dnstt/) | SlowDNS / DNSTT | MIT |
| `badvpn-udpgw.exe` | [badvpn](https://github.com/ambrop72/badvpn) | UDP forwarding over SSH | BSD-3-Clause |
| `wireguard.exe`, `wg.exe` | [WireGuard for Windows](https://www.wireguard.com/install/) | WireGuard tunnels | GPL-2.0 / MIT |

Fetch the ones with official Windows releases:

```powershell
pwsh .\scripts\fetch-sidecars.ps1            # everything available
pwsh .\scripts\fetch-sidecars.ps1 -StoreSafe # skips the GPL-3.0 binary
```

**Nothing here is required.** SSH Direct, SSH-over-TLS, SSH-over-Proxy and the
payload injector are implemented natively in `networld-core`. Engines whose
binary is absent are listed in the UI (Tools → Engines) with the reason, and
the corresponding options are disabled rather than failing at connect time.

**Store builds:** the sandbox cannot install drivers or services, so WireGuard
and TUN mode are hidden when the crate is compiled with `--features store`.
Keep GPL-3.0 binaries out of the Store package.
