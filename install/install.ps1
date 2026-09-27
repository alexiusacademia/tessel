# Installs Tessel on Windows (PowerShell):
#   irm https://storage.googleapis.com/tessel-releases/install.ps1 | iex
# Set $env:TESSEL_VERSION = "v0.1.0" first to install a specific version.
$ErrorActionPreference = "Stop"

$Base = "https://storage.googleapis.com/tessel-releases"
$Dir = if ($env:TESSEL_HOME) { $env:TESSEL_HOME } else { Join-Path $env:LOCALAPPDATA "Tessel" }
$Bin = Join-Path $Dir "bin"

if (-not [Environment]::Is64BitOperatingSystem) { throw "Tessel needs 64-bit Windows" }
$Version = if ($env:TESSEL_VERSION) { $env:TESSEL_VERSION } else {
    try { (Invoke-RestMethod "$Base/latest-windows.txt").Trim() } catch { (Invoke-RestMethod "$Base/latest.txt").Trim() }
}
$File = "tessel-$Version-windows-x64.zip"
Write-Host "Installing Tessel $Version into $Bin"

$Tmp = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid())
New-Item -ItemType Directory -Force $Tmp | Out-Null
try {
    Invoke-WebRequest "$Base/$Version/$File" -OutFile (Join-Path $Tmp $File) -UseBasicParsing
    New-Item -ItemType Directory -Force $Bin | Out-Null
    Expand-Archive (Join-Path $Tmp $File) -DestinationPath $Bin -Force
} finally {
    Remove-Item -Recurse -Force $Tmp
}

# Put the bin folder on the user's PATH for new terminals.
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if (-not (($UserPath -split ";") -contains $Bin)) {
    [Environment]::SetEnvironmentVariable("Path", "$Bin;$UserPath", "User")
    Write-Host "Added $Bin to your PATH (open a new terminal to use it)"
}

Write-Host ""
& (Join-Path $Bin "tessel.exe") --version
Write-Host "Done. Try: tessel run hello.tsl, or just 'tessel' in a folder to open the IDE."
