<#
.SYNOPSIS
  Build and install Coxswain from source on Windows.

.DESCRIPTION
  Run from PowerShell in the unpacked folder:

    powershell -ExecutionPolicy Bypass -File install\install.ps1            # both apps
    powershell -ExecutionPolicy Bypass -File install\install.ps1 -TuiOnly   # terminal app only
    powershell -ExecutionPolicy Bypass -File install\install.ps1 -Yes       # no questions

  Installs to %LOCALAPPDATA%\Programs\Coxswain, adds that folder to your user PATH and creates
  a Start Menu shortcut for the desktop app.
#>
param(
    [switch]$TuiOnly,
    [switch]$Yes
)

$ErrorActionPreference = "Stop"
Set-Location (Split-Path -Parent $PSScriptRoot)
$Dest = Join-Path $env:LOCALAPPDATA "Programs\Coxswain"

function Say($text) { Write-Host $text -ForegroundColor Cyan }
function Info($text) { Write-Host "  $text" }
function Fail($text) { Write-Host "Error: $text" -ForegroundColor Red; exit 1 }
function Have($cmd) { [bool](Get-Command $cmd -ErrorAction SilentlyContinue) }

# Ask politely; Enter means yes.
function Ask($question) {
    if ($Yes) { return $true }
    $reply = Read-Host "  $question [Y/n]"
    return ($reply -eq "" -or $reply -match "^[Yy]")
}

function Refresh-Path {
    $env:Path = [Environment]::GetEnvironmentVariable("Path", "Machine") + ";" +
                [Environment]::GetEnvironmentVariable("Path", "User")
}

# Stop on a failing native command (cargo, npm), which $ErrorActionPreference does not do.
# No param block on purpose: flags like --release must reach $args as plain strings.
function Run {
    $exe, $rest = $args
    & $exe @rest
    if ($LASTEXITCODE -ne 0) { Fail "'$($args -join ' ')' failed (exit code $LASTEXITCODE)." }
}

Say "Installing Coxswain (Windows)"

# ---------------------------------------------------------------- Rust

if (-not (Have cargo)) { Refresh-Path }
if (-not (Have cargo)) {
    Say "Rust is needed to build Coxswain, and it isn't installed yet."
    Info "The official installer (rustup, https://rustup.rs) puts it in your user profile."
    Info "It may also offer to install the Visual Studio C++ Build Tools, which Rust needs"
    Info "on Windows; please accept that. You can remove Rust later with 'rustup self uninstall'."
    if (Ask "Would you like me to install Rust now?") {
        $installer = Join-Path $env:TEMP "rustup-init.exe"
        Invoke-WebRequest "https://win.rustup.rs/x86_64" -OutFile $installer -UseBasicParsing
        & $installer -y --profile minimal
        if ($LASTEXITCODE -ne 0) { Fail "The Rust installer did not finish." }
        Refresh-Path
        $env:Path = (Join-Path $env:USERPROFILE ".cargo\bin") + ";" + $env:Path
    } else {
        Fail "No problem. Install Rust from https://rustup.rs when you're ready, then run this again."
    }
}

# Coxswain uses the 2024 edition, which needs Rust 1.85 or newer.
$minor = [int]((rustc --version) -split " ")[1].Split(".")[1]
if ($minor -lt 85) {
    Say "Your Rust ($(rustc --version)) is older than Coxswain needs (1.85)."
    if ((Have rustup) -and (Ask "Would you like me to update it with 'rustup update stable'?")) {
        Run rustup update stable
    } else {
        Fail "Please update Rust (rustup update) and run this again."
    }
}
Info "Rust: $(rustc --version)"

# ---------------------------------------------------------------- desktop app prerequisites

if (-not $TuiOnly -and -not (Have npm)) {
    Say "The desktop app's interface is built with Node.js, which isn't installed."
    if ((Have winget) -and (Ask "Would you like me to install Node.js LTS with winget?")) {
        winget install --id OpenJS.NodeJS.LTS -e --accept-source-agreements --accept-package-agreements
        Refresh-Path
    }
    if (-not (Have npm)) {
        Info "No worries: install Node.js 20 or newer from https://nodejs.org and run this again"
        Info "for the desktop app. Continuing with the terminal app."
        $TuiOnly = $true
    }
}
# The desktop app renders with WebView2, which ships with Windows 10 and 11.

# ---------------------------------------------------------------- build

Say "Building the terminal app (a few minutes the first time)..."
Run cargo build --release --locked -p coxswain
New-Item -ItemType Directory -Force -Path $Dest | Out-Null
Copy-Item "target\release\coxswain.exe" $Dest -Force
Copy-Item "target\release\cox.exe" $Dest -Force
Info "Installed $Dest\coxswain.exe"

if (-not $TuiOnly) {
    Say "Building the desktop app..."
    Push-Location gui
    try {
        Run npm ci --no-audit --no-fund
        Run npx tauri build --no-bundle
    } finally {
        Pop-Location
    }
    Copy-Item "target\release\coxswain-gui.exe" $Dest -Force
    $shell = New-Object -ComObject WScript.Shell
    $link = $shell.CreateShortcut((Join-Path ([Environment]::GetFolderPath("Programs")) "Coxswain.lnk"))
    $link.TargetPath = Join-Path $Dest "coxswain-gui.exe"
    $link.WorkingDirectory = $env:USERPROFILE
    $link.Save()
    Info "Installed $Dest\coxswain-gui.exe and a Start Menu shortcut"
}

# ---------------------------------------------------------------- PATH

$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if (($userPath -split ";") -notcontains $Dest) {
    [Environment]::SetEnvironmentVariable("Path", "$userPath;$Dest", "User")
    Info "Added $Dest to your PATH (open a new terminal to use it)."
}

Write-Host ""
Say "Done."
Info "Terminal app: coxswain (or cox)        Desktop app: Coxswain in the Start Menu"
Info "Git glyphs look best with a Nerd Font: https://www.nerdfonts.com"
