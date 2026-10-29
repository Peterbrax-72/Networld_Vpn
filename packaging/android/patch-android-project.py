#!/usr/bin/env python3
"""Run after `tauri android init`: copies the Kotlin VPN code into the generated
Android project and declares the VpnService in its manifest."""
import pathlib, re, shutil, sys

root = pathlib.Path("src-tauri/gen/android/app/src/main")
manifest = root / "AndroidManifest.xml"
if not manifest.exists():
    sys.exit(f"missing {manifest} - run `tauri android init` first")

dest = root / "java" / "com" / "networld" / "vpn"
dest.mkdir(parents=True, exist_ok=True)
here = pathlib.Path(__file__).parent
for f in ("NativeBridge.kt", "NetworldVpnService.kt", "VpnPlugin.kt", "MainActivity.kt"):
    shutil.copy(here / f, dest / f)

xml = manifest.read_text()
if "NetworldVpnService" not in xml:
    service = (
        '    <service android:name="com.networld.vpn.NetworldVpnService"\n'
        '        android:exported="false"\n'
        '        android:permission="android.permission.BIND_VPN_SERVICE">\n'
        "        <intent-filter>\n"
        '            <action android:name="android.net.VpnService" />\n'
        "        </intent-filter>\n"
        "    </service>\n"
    )
    xml = xml.replace("</application>", service + "    </application>", 1)
    manifest.write_text(xml)
print("Android VPN sources installed")
