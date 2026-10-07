# Builds Tessel's Windows package on a Windows machine and uploads it to the
# release bucket: what the Release workflow's Windows job does, for when it
# can't run on GitHub.
#
#   git clone https://github.com/alexiusacademia/tessel
#   cd tessel
#   powershell -ExecutionPolicy Bypass -File install\release-windows.ps1
#
# It builds the commit that's checked out, as the version in its Cargo.toml
# (right after a release, that's master). The script makes
# dist\tessel-<version>-windows-x64.zip, tests it, uploads it to
# gs://tessel-releases/<version>/, adds it to that release's SHA256SUMS and
# points latest-windows.txt at it (which is what `tessel update` and the
# install script read).
#
#   -NoUpload      build and test the package only
#   -UploadOnly    upload the package a run with -NoUpload made
#   -Tools <dir>   where LLVM and llvm-mingw are kept between runs
#                  (default: %LOCALAPPDATA%\tessel-build; about 3 GB)
#
# It needs, installed beforehand:
#   - Rust (https://rustup.rs), with Visual Studio's C++ build tools
#     ("Desktop development with C++"), which rustup asks for;
#   - Git for Windows (for git, and the sh that comes with it);
#   - the Google Cloud CLI (https://cloud.google.com/sdk/docs/install),
#     signed in with `gcloud auth login` as an account that can write to the
#     tessel-releases bucket (not needed with -NoUpload).
# LLVM and llvm-mingw are downloaded by the script, the first time.

param(
    [switch]$NoUpload,
    [switch]$UploadOnly,
    [string]$Tools = (Join-Path $env:LOCALAPPDATA "tessel-build")
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

# The same versions as .github/workflows/release.yml.
$LlvmVersion = "22.1.8"
$MingwVersion = "20260922"
$Bucket = "gs://tessel-releases"
$Target = "x86_64-pc-windows-msvc"
$MingwTarget = "x86_64-pc-windows-gnullvm"

function Step($text) { Write-Host ""; Write-Host "==> $text" -ForegroundColor Cyan }
function Fail($text) { Write-Host "error: $text" -ForegroundColor Red; exit 1 }

# Runs a program, and stops the script if it fails. (A plain function, so
# that options like -o go to the program and not to PowerShell.)
function Run {
    $program = $args[0]
    $rest = @($args | Select-Object -Skip 1 | ForEach-Object { $_ })
    & $program @rest
    if ($LASTEXITCODE -ne 0) { Fail "$program $rest failed (exit code $LASTEXITCODE)" }
}

# Runs a program that may fail, and gives what it printed (errors too);
# $LASTEXITCODE says how it went. (Windows PowerShell stops a script whose
# program writes to its error output while that's redirected, unless errors
# are let through like this.)
function Attempt([scriptblock]$block) {
    $old = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try { & $block 2>&1 | ForEach-Object { "$_" } } finally { $ErrorActionPreference = $old }
}

function Need($program, $how) {
    if (-not (Get-Command $program -ErrorAction SilentlyContinue)) { Fail "$program isn't installed (or not on PATH). $how" }
}

# Text without Windows line endings, to compare what programs print.
function Lines($text) { (($text | Out-String) -replace "`r", "").Trim() }

# ---- Where we are, and which version ----

$Repo = Split-Path -Parent $PSScriptRoot
Set-Location $Repo
if (-not (Test-Path "Cargo.toml") -or -not (Test-Path "install\windows-sdk.sh")) {
    Fail "run this from a clone of the Tessel repository (it's install\release-windows.ps1 there)"
}
$match = Select-String -Path "Cargo.toml" -Pattern '^version = "(.*)"' | Select-Object -First 1
if (-not $match) { Fail "can't find the version in Cargo.toml" }
$Version = "v" + $match.Matches[0].Groups[1].Value
$Zip = "tessel-$Version-windows-x64.zip"
$ZipPath = Join-Path $Repo "dist\$Zip"

Write-Host "Tessel $Version for Windows (x64)"
Need "git" "Install Git for Windows: https://git-scm.com/download/win"
$commit = (& git rev-parse --short HEAD)
$tag = (& git tag --points-at HEAD) -join ", "
if ($tag) { Write-Host "Building commit $commit ($tag)" } else {
    Write-Host "Building commit $commit, which has no tag: check it's the commit of the release." -ForegroundColor Yellow
}
if (& git status --porcelain --untracked-files=no) {
    Write-Host "The checkout has changes that aren't committed: the package will include them." -ForegroundColor Yellow
}

# ---- The tools ----

if (-not $NoUpload) {
    Need "gcloud" "Install the Google Cloud CLI: https://cloud.google.com/sdk/docs/install"
    Step "Checking access to $Bucket"
    $said = Attempt { gcloud storage ls "$Bucket/latest-windows.txt" }
    if ($LASTEXITCODE -ne 0) { Fail "can't read $Bucket ($said). Sign in with: gcloud auth login" }
}

if (-not $UploadOnly) {
    Need "cargo" "Install Rust: https://rustup.rs"
    Need "rustup" "Install Rust with rustup: https://rustup.rs"
    Need "tar" "It comes with Windows 10 (1803) and later."
    Need "curl.exe" "It comes with Windows 10 (1803) and later."
    # The sh that comes with Git (install\windows-sdk.sh is a shell script).
    $sh = Join-Path (Split-Path -Parent (Split-Path -Parent (Get-Command git).Source)) "bin\sh.exe"
    if (-not (Test-Path $sh)) {
        $found = Get-Command sh -ErrorAction SilentlyContinue
        if (-not $found) { Fail "can't find sh.exe (it comes with Git for Windows)" }
        $sh = $found.Source
    }

    New-Item -ItemType Directory -Force $Tools | Out-Null
    $llvmName = "clang+llvm-$LlvmVersion-x86_64-pc-windows-msvc"
    $llvm = Join-Path $Tools $llvmName
    if (-not (Test-Path "$llvm\bin\llvm-config.exe")) {
        Step "Downloading LLVM $LlvmVersion (about 1 GB; once)"
        $archive = Join-Path $Tools "llvm.tar.xz"
        Run curl.exe -SfL -o $archive "https://github.com/llvm/llvm-project/releases/download/llvmorg-$LlvmVersion/$llvmName.tar.xz"
        Run tar -xf $archive -C $Tools
        Remove-Item $archive
    }
    $mingwName = "llvm-mingw-$MingwVersion-ucrt-x86_64"
    $mingw = Join-Path $Tools $mingwName
    if (-not (Test-Path "$mingw\bin\x86_64-w64-mingw32-clang.exe")) {
        Step "Downloading llvm-mingw $MingwVersion (once)"
        $archive = Join-Path $Tools "mingw.zip"
        Run curl.exe -SfL -o $archive "https://github.com/mstorsjo/llvm-mingw/releases/download/$MingwVersion/$mingwName.zip"
        Expand-Archive $archive -DestinationPath $Tools -Force
        Remove-Item $archive
    }
    # LLVM lists libxml2 (xml2s.lib) but doesn't ship it; lld's manifest
    # merger uses it. Tessel never merges manifests, so stubs will do.
    if (-not (Test-Path "$llvm\lib\xml2s.lib")) {
        $obj = Join-Path $Tools "xml2-stub.obj"
        Run "$llvm\bin\clang-cl.exe" /c /MT "install\xml2-stub.c" "/Fo:$obj"
        Run "$llvm\bin\llvm-lib.exe" "/out:$llvm\lib\xml2s.lib" $obj
        Remove-Item $obj
    }

    Step "Rust targets"
    Run rustup target add $Target $MingwTarget

    $env:LLVM_SYS_221_PREFIX = $llvm
    # The MinGW runtime library is built with llvm-mingw's clang (for ring's C code).
    $env:CC_x86_64_pc_windows_gnullvm = "$mingw\bin\x86_64-w64-mingw32-clang.exe"
    $env:AR_x86_64_pc_windows_gnullvm = "$mingw\bin\llvm-ar.exe"
    $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_LINKER = "$mingw\bin\x86_64-w64-mingw32-clang.exe"

    # ---- Build ----

    Step "Building tessel (this takes a while the first time)"
    # LLVM's linker is built into tessel (see crates/tessel-cli/build.rs).
    Run cargo build --release --workspace --target $Target --features tessel-cli/bundled-linker
    Step "Building the runtime library apps are linked with"
    Run cargo build --release -p tessel-runtime --target $MingwTarget

    # ---- Package ----

    Step "Packaging"
    if (Test-Path "dist") { Remove-Item -Recurse -Force "dist" }
    New-Item -ItemType Directory -Force "dist\pkg" | Out-Null
    Copy-Item "target\$Target\release\tessel.exe" "dist\pkg\"
    Copy-Item "target\$MingwTarget\release\libtessel_runtime.a" "dist\pkg\"
    # MinGW-w64's C runtime and the Windows import libraries, for the built-in linker.
    Run $sh "install/windows-sdk.sh" ($mingw -replace "\\", "/") "dist/pkg/sdk"
    Copy-Item "install\PACKAGE-README.txt" "dist\pkg\README.txt"

    # ---- Test the package, as the Release workflow does ----

    Step "Testing the package"
    $t = Join-Path $Repo "dist\pkg\tessel.exe"
    Run $t --version
    Run $t check examples/todos
    Run $t run tests/run/hello.tsl
    foreach ($name in "math_calls", "data_append") {
        $got = Lines (& $t run "tests/run/$name.tsl")
        $expected = Lines (Get-Content "tests/run/$name.out" -Raw)
        if ($got -ne $expected) { Fail "tests/run/$name.tsl printed something else than tests/run/$name.out:`n$got" }
        Write-Host "$name ok"
    }
    $answer = Lines ("20 + 22" | & $t repl)
    if ($answer -ne "42") { Fail "the REPL answered '$answer' to 20 + 22" }
    Write-Host "repl ok"
    Run $t build examples/todos -o dist/smoke/todos
    New-Item -ItemType Directory -Force "dist\smoke\hello" | Out-Null
    [IO.File]::WriteAllText((Join-Path $Repo "dist\smoke\hello\main.tsl"), "app Hello {`n    Text(`"hello`")`n}`n")
    Run $t build dist/smoke/hello -o dist/smoke/hello-app
    Write-Host ("A one-Text app is {0} bytes" -f (Get-Item "dist\smoke\hello-app.exe").Length)
    $env:TESSEL_SCRIPT = 'click field 1; type "hello"; click "Add"; tree'
    # (An app has no console: its output is read through a pipe.)
    $tree = Lines (& "dist\smoke\todos.exe" | Out-String)
    Remove-Item Env:TESSEL_SCRIPT
    if ($tree -notmatch "hello") { Fail "the todos example didn't run as expected:`n$tree" }
    Write-Host "todos ok"

    # ---- Archive ----

    Step "Making $Zip"
    # (tar makes a zip with / in its paths, which every unzipper reads.)
    Push-Location "dist\pkg"
    $items = @(Get-ChildItem -Name)
    Run tar -a -c -f "..\$Zip" $items
    Pop-Location
    Write-Host ("dist\$Zip: {0:N1} MB" -f ((Get-Item $ZipPath).Length / 1MB))
}

if ($NoUpload) {
    Write-Host ""
    Write-Host "Built dist\$Zip. Upload it with: install\release-windows.ps1 -UploadOnly" -ForegroundColor Green
    exit 0
}

# ---- Upload ----

if (-not (Test-Path $ZipPath)) { Fail "there's no dist\$Zip to upload: build it first (run without -UploadOnly)" }
Step "Uploading to $Bucket/$Version/"
$work = Join-Path $Repo "dist\upload"
if (Test-Path $work) { Remove-Item -Recurse -Force $work }
New-Item -ItemType Directory -Force $work | Out-Null

# The release's checksums, with this package's added (or replaced). The
# macOS package's line, if the release has one, stays.
$sumsFile = Join-Path $work "SHA256SUMS"
$lines = @()
# (A release that has no package yet has no SHA256SUMS: that's fine.)
Attempt { gcloud storage cp "$Bucket/$Version/SHA256SUMS" $sumsFile } | Out-Null
if ($LASTEXITCODE -eq 0 -and (Test-Path $sumsFile)) {
    $lines = @(Get-Content $sumsFile | Where-Object { $_.Trim() -and ($_ -notmatch [regex]::Escape($Zip)) })
}
$hash = (Get-FileHash $ZipPath -Algorithm SHA256).Hash.ToLower()
$lines += "$hash  $Zip"
# (Unix line endings and no byte-order mark: `tessel update` and sha256sum read it.)
[IO.File]::WriteAllText($sumsFile, (($lines -join "`n") + "`n"), (New-Object Text.UTF8Encoding($false)))

Run gcloud storage cp $ZipPath "$Bucket/$Version/"
Run gcloud storage cp $sumsFile "$Bucket/$Version/"

# latest-windows.txt: the version `tessel update` and the install script
# take. It isn't moved back from a newer release.
$latest = Lines (& curl.exe -sf "https://storage.googleapis.com/tessel-releases/latest-windows.txt")
$newer = $false
try { $newer = [version]($latest.TrimStart("v")) -gt [version]($Version.TrimStart("v")) } catch { }
if ($newer) {
    Write-Host "latest-windows.txt stays at $latest, which is newer than $Version." -ForegroundColor Yellow
} else {
    $pointer = Join-Path $work "latest-windows.txt"
    [IO.File]::WriteAllText($pointer, "$Version`n", (New-Object Text.UTF8Encoding($false)))
    Run gcloud storage cp $pointer (Join-Path $Repo "install\install.ps1") "$Bucket/" "--cache-control=no-cache, max-age=0"
}
Remove-Item -Recurse -Force $work

Step "Checking what's published"
$published = Lines (& curl.exe -sf "https://storage.googleapis.com/tessel-releases/latest-windows.txt")
# (From the bucket itself: the public address may show an older copy for an hour.)
$sums = Lines (Attempt { gcloud storage cat "$Bucket/$Version/SHA256SUMS" })
Write-Host "latest-windows.txt: $published"
Write-Host $sums
if ($sums -notmatch $hash) { Fail "the published SHA256SUMS doesn't have this package's checksum" }
Write-Host ""
Write-Host "Published Tessel $Version for Windows: https://storage.googleapis.com/tessel-releases/$Version/$Zip" -ForegroundColor Green
