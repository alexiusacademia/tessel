# Checks the installer `tessel build --bundle` makes for a Windows app, on
# Windows: it installs the app without asking (/S), with its shortcut, its
# entry in Settings' installed apps and its document type, the installed
# app runs, and its uninstaller removes all of it again.
#
#   check-setup.ps1 -Setup "Smoke Notes Setup.exe" -Name "Smoke Notes" -Id dev.tessel.smokenotes -Extension note
#
# Author: Alexius Academia
# Date: 2026-10-24
param(
    [Parameter(Mandatory)] [string] $Setup,
    [Parameter(Mandatory)] [string] $Name,
    [Parameter(Mandatory)] [string] $Id,
    [Parameter(Mandatory)] [string] $Extension
)
$ErrorActionPreference = "Stop"

function Check($ok, $what) {
    if (-not $ok) { throw "installer check failed: $what" }
    Write-Host "ok: $what"
}

$dir = Join-Path $env:LOCALAPPDATA "Programs\$Name"
$exe = Join-Path $dir "$Name.exe"
$uninstaller = Join-Path $dir "uninstall.exe"
$key = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\$Id"
$link = Join-Path ([Environment]::GetFolderPath("Programs")) "$Name.lnk"
$progId = "$Id.$Extension"

# Install.
$p = Start-Process -FilePath (Resolve-Path $Setup) -ArgumentList "/S" -Wait -PassThru
Check ($p.ExitCode -eq 0) "the installer exits with 0 (it exited with $($p.ExitCode))"
Check (Test-Path $exe) "the app is in $dir"
Check (Test-Path $uninstaller) "the uninstaller is next to it"
Check (Test-Path $link) "the Start menu has a shortcut"
$entry = Get-ItemProperty $key
Check ($entry.DisplayName -eq $Name) "Settings lists the app"
Check ($entry.UninstallString -like "*uninstall.exe*") "Settings knows how to remove it"
$type = (Get-ItemProperty "HKCU:\Software\Classes\.$Extension").'(default)'
Check ($type -eq $progId) ".$Extension files open with the app"
$command = (Get-ItemProperty "HKCU:\Software\Classes\$progId\shell\open\command").'(default)'
Check ($command -like "*$Name.exe*%1*") "the app is given the file to open"

# The installed app runs.
$env:TESSEL_SCRIPT = "tree"
$out = Join-Path $env:TEMP "setup-check-out.txt"
$p = Start-Process -FilePath $exe -Wait -PassThru -NoNewWindow -RedirectStandardOutput $out
Remove-Item Env:\TESSEL_SCRIPT
Check ($p.ExitCode -eq 0) "the installed app runs"
Get-Content $out

# Installing again (an update) works, even while the app is running.
$env:TESSEL_SCRIPT = "wait 30"
$running = Start-Process -FilePath $exe -PassThru
Remove-Item Env:\TESSEL_SCRIPT
Start-Sleep -Seconds 2
Check (-not $running.HasExited) "the app is running"
$p = Start-Process -FilePath (Resolve-Path $Setup) -ArgumentList "/S" -Wait -PassThru
Check ($p.ExitCode -eq 0) "installing again works while the app is running"
Check (Test-Path $exe) "the app is there after the update"
Stop-Process -Id $running.Id -Force
$running.WaitForExit()

# Uninstall, the way Settings does (quietly).
$p = Start-Process -FilePath $uninstaller -ArgumentList "/uninstall", "/S" -Wait -PassThru
Check ($p.ExitCode -eq 0) "the uninstaller exits with 0 (it exited with $($p.ExitCode))"
Check (-not (Test-Path $exe)) "the app is removed"
Check (-not (Test-Path $link)) "the shortcut is removed"
Check (-not (Test-Path $key)) "Settings no longer lists it"
Check (-not (Test-Path "HKCU:\Software\Classes\$progId")) "its document type is removed"
# The folder goes once the uninstaller has quit.
for ($i = 0; $i -lt 20 -and (Test-Path $dir); $i++) { Start-Sleep -Milliseconds 500 }
Check (-not (Test-Path $dir)) "its folder is removed"
