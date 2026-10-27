<#
.SYNOPSIS
    Creates a self-signed code-signing certificate for sideload-testing the
    Networld VPN MSIX before (or alongside) Store submission.

.NOTES
    The -Subject MUST match the Publisher value in AppxManifest.xml exactly,
    otherwise Windows refuses to install the package.
#>
[CmdletBinding()]
param(
    [string]$Subject = "CN=Networld, O=Networld, C=EE",
    [string]$OutPfx = "$PSScriptRoot\NetworldTest.pfx",
    [int]$ValidYears = 3
)

$ErrorActionPreference = "Stop"

$password = Read-Host "Password for the .pfx" -AsSecureString

$cert = New-SelfSignedCertificate `
    -Type Custom `
    -Subject $Subject `
    -KeyUsage DigitalSignature `
    -FriendlyName "Networld VPN (test signing)" `
    -CertStoreLocation "Cert:\CurrentUser\My" `
    -NotAfter (Get-Date).AddYears($ValidYears) `
    -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3", "2.5.29.19={text}")

Export-PfxCertificate -Cert "Cert:\CurrentUser\My\$($cert.Thumbprint)" -FilePath $OutPfx -Password $password | Out-Null
$cerPath = [IO.Path]::ChangeExtension($OutPfx, ".cer")
Export-Certificate -Cert "Cert:\CurrentUser\My\$($cert.Thumbprint)" -FilePath $cerPath | Out-Null

Write-Host @"

Created:
  $OutPfx   (sign with this)
  $cerPath  (install into 'Trusted People' on test machines)

Trust it locally with an elevated prompt:
  Import-Certificate -FilePath "$cerPath" -CertStoreLocation Cert:\LocalMachine\TrustedPeople

Then build a signed package:
  .\Build-Msix.ps1 -Target Sideload -CertPath "$OutPfx" -CertPassword (Read-Host -AsSecureString)

Remember to set Publisher="$Subject" in AppxManifest.xml for sideload builds.
"@ -ForegroundColor Green
