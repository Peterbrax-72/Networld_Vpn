<#
.SYNOPSIS
    Builds Networld VPN and packages it as an MSIX for the Microsoft Store
    (or for signed sideloading).

.DESCRIPTION
    Pipeline:
      1. npm ci + npm run build          — React/Vite front-end → dist/
      2. cargo build --release           — Tauri shell + networld-core → .exe
      3. stage/                          — exe + Assets + resources/bin + manifest
      4. makepri                         — resources.pri from priconfig.xml
      5. makeappx pack                   — stage/ → NetworldVPN_<ver>_x64.msix
      6. signtool (sideload only)        — Store packages are signed by Microsoft

.EXAMPLE
    # Store submission package
    .\Build-Msix.ps1 -Version 1.0.0.0 `
        -IdentityName "12345Networld.NetworldVPN" `
        -Publisher "CN=A1B2C3D4-5E6F-7890-ABCD-EF1234567890" `
        -PublisherDisplayName "Networld"

.EXAMPLE
    # Locally installable test build
    .\Build-Msix.ps1 -Target Sideload -CertPath .\NetworldTest.pfx -CertPassword (Read-Host -AsSecureString)
#>
[CmdletBinding()]
param(
    [string]$Version = "1.0.0.0",
    [ValidateSet("Store", "Sideload")] [string]$Target = "Store",
    [string]$IdentityName,
    [string]$Publisher,
    [string]$PublisherDisplayName,
    [string]$CertPath,
    [System.Security.SecureString]$CertPassword,
    [string]$TimestampUrl = "http://timestamp.digicert.com",
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$root     = (Resolve-Path "$PSScriptRoot\..\..").Path
$stage    = Join-Path $root "packaging\msix\stage"
$outDir   = Join-Path $root "packaging\msix\out"
$assets   = Join-Path $root "packaging\msix\Assets"
$manifest = Join-Path $root "packaging\msix\AppxManifest.xml"
$exeName  = "NetworldVPN.exe"

function Write-Step($msg) { Write-Host "`n=== $msg" -ForegroundColor Cyan }

function Find-SdkTool([string]$name) {
    $roots = @(
        "${env:ProgramFiles(x86)}\Windows Kits\10\bin",
        "${env:ProgramFiles}\Windows Kits\10\bin"
    ) | Where-Object { Test-Path $_ }

    $tool = Get-ChildItem -Path $roots -Recurse -Filter $name -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match "\\(x64|x86)\\" } |
        Sort-Object FullName -Descending | Select-Object -First 1

    if (-not $tool) {
        throw "$name not found. Install the Windows 10/11 SDK (winget install Microsoft.WindowsSDK)."
    }
    return $tool.FullName
}

# ---------------------------------------------------------------- 1 & 2 build
if (-not $SkipBuild) {
    Write-Step "Building the front-end"
    Push-Location $root
    if (Test-Path "package-lock.json") { npm ci } else { npm install }
    npm run build
    Pop-Location

    Write-Step "Building the Rust binary (release$(if ($Target -eq 'Store') { ', feature: store' }))"
    Push-Location (Join-Path $root "src-tauri")
    # custom-protocol embeds the UI in the exe; without it the app loads localhost:1420 and shows nothing.
    $features = if ($Target -eq "Store") { @("--features", "store,custom-protocol") } else { @("--features", "custom-protocol") }
    cargo build --release @features
    Pop-Location
}

$builtExe = Join-Path $root "target\release\networld-vpn.exe"
if (-not (Test-Path $builtExe)) {
    $builtExe = Join-Path $root "src-tauri\target\release\networld-vpn.exe"
}
if (-not (Test-Path $builtExe)) { throw "Could not find networld-vpn.exe — run without -SkipBuild." }

# ------------------------------------------------------------------ 3 stage
Write-Step "Staging the package layout"
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Path $stage | Out-Null
New-Item -ItemType Directory -Path $outDir -Force | Out-Null

Copy-Item $builtExe (Join-Path $stage $exeName)
Copy-Item $assets (Join-Path $stage "Assets") -Recurse

# Sidecar engines (xray, tun2socks, dnstt-client …) ship as plain files.
$binSrc = Join-Path $root "src-tauri\resources\bin"
if (Test-Path $binSrc) {
    $binDst = Join-Path $stage "resources\bin"
    New-Item -ItemType Directory -Path $binDst -Force | Out-Null
    Copy-Item "$binSrc\*" $binDst -Recurse -Force
    Write-Host "  bundled sidecars: $((Get-ChildItem $binDst).Name -join ', ')"
} else {
    Write-Warning "resources\bin is empty — V2Ray/Xray, SlowDNS and TUN mode will report 'engine binary missing'. Run scripts\fetch-sidecars.ps1 first."
}

# Patch the manifest copy that goes into the package.
[xml]$xml = Get-Content $manifest
$ns = New-Object System.Xml.XmlNamespaceManager($xml.NameTable)
$ns.AddNamespace("d", "http://schemas.microsoft.com/appx/manifest/foundation/windows10")
$identity = $xml.SelectSingleNode("//d:Identity", $ns)
$identity.Version = $Version
if ($IdentityName)        { $identity.Name = $IdentityName }
if ($Publisher)           { $identity.Publisher = $Publisher }
if ($PublisherDisplayName) {
    $xml.SelectSingleNode("//d:Properties/d:PublisherDisplayName", $ns).InnerText = $PublisherDisplayName
}
$displayName = $xml.SelectSingleNode("//d:Properties/d:PublisherDisplayName", $ns).InnerText
$placeholder = ($identity.Name -like "*REPLACE-ME*") -or ($identity.Publisher -like "*REPLACE-ME*") -or ($displayName -like "*REPLACE-ME*")
if ($placeholder) {
    if ($Target -eq "Store") {
        throw "The manifest still contains REPLACE-ME (Identity Name, Publisher or PublisherDisplayName). Set the real Partner Center values first."
    }
    Write-Warning "The manifest still contains REPLACE-ME. Fine for a sideload test, but Partner Center would reject it."
}
if ($Target -eq "Store" -and $Version.Split('.')[3] -ne "0") {
    throw "Store packages must have revision 0 (got $Version). Use a version like 1.0.5.0."
}
$xml.Save((Join-Path $stage "AppxManifest.xml"))
Write-Host "  identity : $($identity.Name)"
Write-Host "  publisher: $($identity.Publisher)"
Write-Host "  version  : $($identity.Version)"

# ------------------------------------------------------------------- 4 pri
Write-Step "Generating resources.pri"
$makepri = Find-SdkTool "makepri.exe"
Push-Location $stage
& $makepri new /pr $stage /cf (Join-Path $root "packaging\msix\priconfig.xml") /of (Join-Path $stage "resources.pri") /o
$priExit = $LASTEXITCODE
Pop-Location
if ($priExit -ne 0 -or -not (Test-Path (Join-Path $stage "resources.pri"))) {
    throw "makepri failed (exit $priExit) - the package would ship without resources.pri"
}

# ------------------------------------------------------------------ 5 pack
Write-Step "Packing the MSIX"
$makeappx = Find-SdkTool "makeappx.exe"
$msix = Join-Path $outDir "NetworldVPN_${Version}_x64.msix"
& $makeappx pack /d $stage /p $msix /o
if ($LASTEXITCODE -ne 0) { throw "makeappx failed" }

# ------------------------------------------------------------------ 6 sign
if ($Target -eq "Sideload") {
    if (-not $CertPath) { throw "-CertPath is required for a sideload package." }
    Write-Step "Signing"
    $signtool = Find-SdkTool "signtool.exe"
    $plain = [Runtime.InteropServices.Marshal]::PtrToStringAuto(
        [Runtime.InteropServices.Marshal]::SecureStringToBSTR($CertPassword))
    & $signtool sign /fd SHA256 /a /f $CertPath /p $plain /tr $TimestampUrl /td SHA256 $msix
    if ($LASTEXITCODE -ne 0) { throw "signtool failed" }
} else {
    Write-Host "`nStore target: the package is intentionally unsigned — Partner Center signs it." -ForegroundColor Yellow
}

Write-Step "Done"
Write-Host "  $msix" -ForegroundColor Green
Write-Host @"

Next steps (Store):
  1. Partner Center → Apps and games → your app → Packages → upload the .msix
  2. Keep the Identity/Publisher values identical to Product identity
  3. Submission notes: the app bundles the Xray/tun2socks engines as plain
     executables; no drivers or services are installed by the Store build.
"@
