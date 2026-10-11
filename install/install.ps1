# Installs Tessel on Windows (PowerShell):
#   irm https://storage.googleapis.com/tessel-releases/install.ps1 | iex
# Set $env:TESSEL_VERSION = "v0.1.0" first to install a specific version, and
# $env:TESSEL_NO_STATS = "1" to leave the download and install uncounted.
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

# Counts the download and the install (or the update, over an earlier
# install): the version, the system, and a random id kept in <folder>\stats,
# and nothing else. See "What Tessel reports" in the installation guide. It
# never holds up or fails the install. `tessel` reads and writes the same
# file (its `telemetry` says what's in it), so its other lines are kept.
$Told = $false
$Track = (-not $env:TESSEL_NO_STATS) -and ((-not $env:DO_NOT_TRACK) -or ($env:DO_NOT_TRACK -eq "0"))
if ($Track) {
    try {
        $Ping = if ($env:TESSEL_STATS_URL) { $env:TESSEL_STATS_URL } else { "https://stats.syncster.dev/v1/ping" }
        $V = $Version.TrimStart("v")
        $Stats = Join-Path $Dir "stats"
        $Id = ""; $Old = ""; $Off = ""; $Other = @()
        if (Test-Path $Stats) {
            foreach ($Line in Get-Content $Stats) {
                if ($Line.StartsWith("id=")) { $Id = $Line.Substring(3) }
                elseif ($Line.StartsWith("reported=")) { $Old = $Line.Substring(9) }
                elseif ($Line.StartsWith("noticed=")) { }
                else {
                    if ($Line -eq "off=1") { $Off = "1" }
                    if ($Line) { $Other += $Line }
                }
            }
        }
        # (Turned off with `tessel telemetry off`: it stays off.)
        if (-not $Off) {
            $Told = $true
            function Save($Reported) { Set-Content $Stats (($Other + @("id=$Id", "reported=$Reported", "noticed=1")) -join "`n") }
            try { Invoke-WebRequest "$Ping`?app=tessel&type=download&v=$V&os=windows" -UseBasicParsing -TimeoutSec 5 | Out-Null } catch {}
            if (-not $Id) { $Id = [Guid]::NewGuid().ToString() }
            Save $Old
            if ($Old -ne $V) {
                $Kind = if ($Old) { "update" } else { "install" }
                $Prev = if ($Old) { "&prev=$Old" } else { "" }
                # The version is noted only once the count has taken it, so one
                # that didn't arrive is sent by the next install or `tessel update`.
                Invoke-WebRequest "$Ping`?app=tessel&type=$Kind&id=$Id&v=$V&os=windows&arch=x86_64$Prev" -UseBasicParsing -TimeoutSec 5 | Out-Null
                Save $V
            }
        }
    } catch {}
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
if ($Told) {
    Write-Host ""
    Write-Host "Tessel reports that it was installed, and at most once a day that it's in use:"
    Write-Host "its version, your system, and a random id for this installation. Nothing about"
    Write-Host "you, your files or your code. 'tessel telemetry off' turns it off. More:"
    Write-Host "https://tessel-lang.syncster.dev/start/installation/#what-tessel-reports"
}
