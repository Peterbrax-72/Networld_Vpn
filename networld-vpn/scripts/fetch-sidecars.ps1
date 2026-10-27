<#
.SYNOPSIS
    Downloads the optional engine sidecars into src-tauri\resources\bin.

.DESCRIPTION
    Networld VPN ships its SSH / SSH-over-TLS / payload engine natively in
    Rust — those work with no sidecars at all. The remaining engines shell out
    to well-known upstream binaries:

        xray.exe           Xray-core            VMess / VLESS / Trojan / Shadowsocks
        tun2socks.exe      xjasonlyu/tun2socks  system-wide TUN mode
        dnstt-client.exe   dnstt (SlowDNS)      DNS tunnelling
        badvpn-udpgw.exe   badvpn               UDP over the SSH tunnel

    Anything missing simply shows as "engine binary missing" in the UI; the
    app still runs.

.NOTES
    Check each project's licence before redistributing it in a Store package.
    Xray-core is MPL-2.0, tun2socks is GPL-3.0, badvpn is BSD-3-Clause.
    GPL-3.0 binaries and the Microsoft Store are a poor fit — ship tun2socks
    only in the sideload/NSIS build (the Store build hides TUN mode anyway).

.EXAMPLE
    pwsh .\scripts\fetch-sidecars.ps1
    pwsh .\scripts\fetch-sidecars.ps1 -Only xray -StoreSafe
#>
[CmdletBinding()]
param(
    [ValidateSet("all", "xray", "tun2socks", "dnstt", "udpgw")] [string]$Only = "all",
    [switch]$StoreSafe,
    [switch]$Force
)

$ErrorActionPreference = "Stop"
$root = (Resolve-Path "$PSScriptRoot\..").Path
$bin  = Join-Path $root "src-tauri\resources\bin"
New-Item -ItemType Directory -Path $bin -Force | Out-Null

$XRAY_VERSION      = "v25.8.3"
$TUN2SOCKS_VERSION = "v2.6.0"

function Get-Zip([string]$url, [string]$dest) {
    $tmp = Join-Path $env:TEMP ("nw_" + [IO.Path]::GetRandomFileName() + ".zip")
    Write-Host "  ↓ $url"
    Invoke-WebRequest -Uri $url -OutFile $tmp -UseBasicParsing
    $ex = Join-Path $env:TEMP ("nw_" + [IO.Path]::GetRandomFileName())
    Expand-Archive -Path $tmp -DestinationPath $ex -Force
    Get-ChildItem $ex -Recurse -Filter *.exe | ForEach-Object {
        Copy-Item $_.FullName (Join-Path $dest $_.Name) -Force
        Write-Host "  + $($_.Name)" -ForegroundColor Green
    }
    # Xray needs its geo databases next to the executable
    Get-ChildItem $ex -Recurse -Include *.dat | ForEach-Object {
        Copy-Item $_.FullName (Join-Path $dest $_.Name) -Force
        Write-Host "  + $($_.Name)" -ForegroundColor Green
    }
    Remove-Item $tmp, $ex -Recurse -Force -ErrorAction SilentlyContinue
}

function Need([string]$name, [string]$file) {
    if ($Only -ne "all" -and $Only -ne $name) { return $false }
    if ((Test-Path (Join-Path $bin $file)) -and -not $Force) {
        Write-Host "  = $file already present (use -Force to refresh)" -ForegroundColor DarkGray
        return $false
    }
    return $true
}

Write-Host "Fetching sidecars into $bin" -ForegroundColor Cyan

if (Need "xray" "xray.exe") {
    Get-Zip "https://github.com/XTLS/Xray-core/releases/download/$XRAY_VERSION/Xray-windows-64.zip" $bin
}

if (Need "tun2socks" "tun2socks.exe") {
    if ($StoreSafe) {
        Write-Host "  ! skipping tun2socks (GPL-3.0) because -StoreSafe was passed" -ForegroundColor Yellow
    } else {
        Get-Zip "https://github.com/xjasonlyu/tun2socks/releases/download/$TUN2SOCKS_VERSION/tun2socks-windows-amd64.zip" $bin
        Get-ChildItem $bin -Filter "tun2socks-windows-amd64*" | Rename-Item -NewName "tun2socks.exe" -Force -ErrorAction SilentlyContinue
    }
}

if (Need "dnstt" "dnstt-client.exe") {
    Write-Host @"
  ! dnstt-client has no official Windows release. Build it yourself:
        go install www.bamsoftware.com/git/dnstt.git/dnstt-client@latest
    then copy %USERPROFILE%\go\bin\dnstt-client.exe into
        $bin
"@ -ForegroundColor Yellow
}

if (Need "udpgw" "badvpn-udpgw.exe") {
    Write-Host @"
  ! badvpn-udpgw has no official Windows release either. Either build badvpn
    with CMake, or point Settings → DNS & UDPGW at a remote udpgw endpoint
    (127.0.0.1:7300 is only used when a local binary exists).
"@ -ForegroundColor Yellow
}

Write-Host "`nContents of resources\bin:" -ForegroundColor Cyan
Get-ChildItem $bin | Select-Object Name, @{n = "MB"; e = { [math]::Round($_.Length / 1MB, 1) } } | Format-Table
