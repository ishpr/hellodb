# hellodb installer for Windows (PowerShell 5+)
#
# One-liner:
#     iwr -useb hellodb.dev/install.ps1 | iex
#
# What it does:
#   1. Detects architecture (x86_64 or aarch64).
#   2. Resolves the latest release tag on GitHub.
#   3. Downloads the matching tarball + SHA256, verifies.
#   4. Extracts to $env:USERPROFILE\.hellodb\ (or $env:HELLODB_HOME).
#   5. Adds the install bin dir to the user's PATH via the registry
#      so it persists across sessions.
#   6. Runs `hellodb.exe init` to bootstrap the encrypted DB.
#   7. Registers the Claude Code plugin if claude.exe is available.
#
# Environment overrides:
#   $env:HELLODB_VERSION        Pin to a tag (default: latest release)
#   $env:HELLODB_INSTALL_DIR    Binary install dir (default: ~\.hellodb\bin)
#   $env:HELLODB_REPO           Source repo (default: ishpr/hellodb)
#   $env:HELLODB_HOME           Data dir (default: ~\.hellodb)
#   $env:HELLODB_SKIP_INIT      "1" to skip `hellodb init`
#   $env:HELLODB_SKIP_PLUGIN    "1" to skip Claude Code plugin registration
#   $env:HELLODB_SKIP_CODEX     "1" to skip OpenAI Codex MCP registration

$ErrorActionPreference = "Stop"
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

function Info($msg) { Write-Host "==> $msg" -ForegroundColor Cyan }
function Ok($msg)   { Write-Host "✓  $msg"  -ForegroundColor Green }
function Warn($msg) { Write-Host "!  $msg"  -ForegroundColor Yellow }
function Fail($msg) { Write-Host "✗  $msg"  -ForegroundColor Red; exit 1 }

# ----- resolve config -----------------------------------------------------

$Repo    = if ($env:HELLODB_REPO)    { $env:HELLODB_REPO }    else { "ishpr/hellodb" }
$Version = if ($env:HELLODB_VERSION) { $env:HELLODB_VERSION } else { "latest" }
$Home_   = if ($env:HELLODB_HOME)    { $env:HELLODB_HOME }    else { Join-Path $env:USERPROFILE ".hellodb" }

$DefaultInstallDir = Join-Path $Home_ "bin"
$InstallDir = if ($env:HELLODB_INSTALL_DIR) { $env:HELLODB_INSTALL_DIR } else { $DefaultInstallDir }

# ----- platform detection -------------------------------------------------

$ArchVar = $env:PROCESSOR_ARCHITECTURE
$Target = switch ($ArchVar) {
  "AMD64" { "x86_64-pc-windows-msvc" }
  "ARM64" { "aarch64-pc-windows-msvc" }
  default { Fail "unsupported architecture: $ArchVar" }
}
Info "detected platform: $Target"

# ----- resolve version ----------------------------------------------------

$Api = "https://api.github.com/repos/$Repo"
if ($Version -eq "latest") {
  Info "resolving latest release from $Api..."
  try {
    $latest = Invoke-RestMethod -Uri "$Api/releases/latest" -Headers @{ "User-Agent" = "hellodb-installer" }
  } catch {
    Fail "couldn't resolve latest release: $_"
  }
  $Tag = $latest.tag_name
} else {
  $Tag = $Version
}
Ok "installing $Tag"

# ----- download + verify --------------------------------------------------

$Tarball = "hellodb-plugin-$Target.tar.gz"
$Url     = "https://github.com/$Repo/releases/download/$Tag/$Tarball"
$ShaUrl  = "$Url.sha256"

$Tmp = Join-Path $env:TEMP "hellodb-install-$([guid]::NewGuid())"
New-Item -ItemType Directory -Force -Path $Tmp | Out-Null
try {
  $TarballPath = Join-Path $Tmp $Tarball
  $ShaPath     = "$TarballPath.sha256"

  Info "downloading $Tarball..."
  try {
    Invoke-WebRequest -Uri $Url -OutFile $TarballPath -UseBasicParsing -Headers @{ "User-Agent" = "hellodb-installer" }
  } catch {
    Fail "download failed. check $Url manually. ($_)"
  }

  Info "verifying checksum..."
  try {
    Invoke-WebRequest -Uri $ShaUrl -OutFile $ShaPath -UseBasicParsing -Headers @{ "User-Agent" = "hellodb-installer" } | Out-Null
    $expected = (Get-Content $ShaPath -Raw).Trim().Split()[0].ToLower()
    $actual   = (Get-FileHash $TarballPath -Algorithm SHA256).Hash.ToLower()
    if ($expected -ne $actual) {
      Fail "SHA256 mismatch. expected $expected, got $actual. aborting."
    }
    Ok "checksum verified"
  } catch {
    Warn "checksum file missing or unreadable — continuing without verify ($_)"
  }

  # ----- extract ---------------------------------------------------------

  $OutDir = Join-Path $Tmp "out"
  New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
  Info "extracting..."
  & tar.exe -xzf $TarballPath -C $OutDir
  if ($LASTEXITCODE -ne 0) {
    Fail "tar extraction failed (exit $LASTEXITCODE). does your Windows have tar? (Windows 10+ ships with it.)"
  }
  $BinSrc = Join-Path $OutDir "plugin\bin"
  if (-not (Test-Path (Join-Path $BinSrc "hellodb.exe"))) {
    Fail "tarball layout unexpected: no plugin\bin\hellodb.exe"
  }

  # ----- install binaries ------------------------------------------------

  New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
  Info "installing binaries to $InstallDir\..."
  foreach ($exe in @("hellodb.exe", "hellodb-mcp.exe", "hellodb-brain.exe")) {
    Copy-Item -Force -Path (Join-Path $BinSrc $exe) -Destination (Join-Path $InstallDir $exe)
  }
  Ok "installed: hellodb.exe, hellodb-mcp.exe, hellodb-brain.exe"

  # ----- copy plugin bundle + marketplace manifest ----------------------
  # Layout must mirror install.sh:
  #   $Home_\plugin\              (the plugin itself)
  #   $Home_\.claude-plugin\
  #     marketplace.json          (manifest `claude plugin marketplace add` looks for)

  $PluginDest = Join-Path $Home_ "plugin"
  if (Test-Path $PluginDest) { Remove-Item -Recurse -Force $PluginDest }
  Copy-Item -Recurse -Force -Path (Join-Path $OutDir "plugin") -Destination $PluginDest
  # Ensure .exe binaries are in the plugin bin/ too
  $PluginBin = Join-Path $PluginDest "bin"
  foreach ($exe in @("hellodb.exe", "hellodb-mcp.exe", "hellodb-brain.exe")) {
    Copy-Item -Force -Path (Join-Path $BinSrc $exe) -Destination (Join-Path $PluginBin $exe)
  }

  $MarketplaceSrc = Join-Path $OutDir ".claude-plugin"
  if (Test-Path $MarketplaceSrc) {
    $MarketplaceDest = Join-Path $Home_ ".claude-plugin"
    if (Test-Path $MarketplaceDest) { Remove-Item -Recurse -Force $MarketplaceDest }
    Copy-Item -Recurse -Force -Path $MarketplaceSrc -Destination $MarketplaceDest
  } else {
    Warn "marketplace.json not found in tarball — plugin registration will need a manual marketplace add."
  }

  # ----- PATH update (persistent, user scope) ---------------------------

  $userPath = [Environment]::GetEnvironmentVariable("PATH", "User")
  if (-not ($userPath -split ";" | Where-Object { $_ -eq $InstallDir })) {
    $newPath = if ([string]::IsNullOrEmpty($userPath)) { $InstallDir } else { "$InstallDir;$userPath" }
    [Environment]::SetEnvironmentVariable("PATH", $newPath, "User")
    $env:PATH = "$InstallDir;$env:PATH"
    Ok "added $InstallDir to your user PATH (restart your terminal to pick it up everywhere)"
  } else {
    Ok "$InstallDir already on user PATH"
  }

  # ----- hellodb init ---------------------------------------------------

  if ($env:HELLODB_SKIP_INIT -eq "1") {
    Info "skipping hellodb init (HELLODB_SKIP_INIT=1)"
  } else {
    Info "bootstrapping hellodb..."
    & (Join-Path $InstallDir "hellodb.exe") init | Out-Null
    Ok "identity + encrypted DB + brain.toml written to $Home_"
  }

  # ----- Claude Code plugin registration --------------------------------

  if ($env:HELLODB_SKIP_PLUGIN -eq "1") {
    Info "skipping Claude Code plugin registration (HELLODB_SKIP_PLUGIN=1)"
  } elseif (Get-Command claude -ErrorAction SilentlyContinue) {
    Info "registering plugin with Claude Code..."
    # $Home_ is the install root: contains both plugin\ and .claude-plugin\marketplace.json
    $markRoot = $Home_
    # Tight presence check: line must be literally "❯ hellodb" (with optional
    # whitespace), not any substring that happens to contain "hellodb".
    $existingMarket = (claude plugin marketplace list 2>$null) -match '^\s*❯\s+hellodb\s*$'
    if (-not $existingMarket) {
      try { claude plugin marketplace add $markRoot 2>&1 | Out-Null; Ok "marketplace added" } catch { Warn "marketplace add failed: $_" }
    } else {
      Ok "marketplace 'hellodb' already registered"
    }
    $existingPlugin = (claude plugin list 2>$null) -match '^\s*❯\s+hellodb@hellodb(\s|$)'
    if (-not $existingPlugin) {
      try { claude plugin install "hellodb@hellodb" 2>&1 | Out-Null; Ok "plugin installed" } catch { Warn "plugin install failed: $_" }
    } else {
      Ok "plugin already installed"
    }
  } else {
    Warn "Claude Code CLI not found; skipping plugin registration."
    Warn "install Claude Code, then run: claude plugin install hellodb@hellodb"
  }

  # ----- OpenAI Codex MCP (stdio) ---------------------------------------

  if ($env:HELLODB_SKIP_CODEX -eq "1") {
    Info "skipping Codex MCP registration (HELLODB_SKIP_CODEX=1)"
  } elseif (Get-Command codex -ErrorAction SilentlyContinue) {
    $McpExe = Join-Path $InstallDir "hellodb-mcp.exe"
    & codex mcp get hellodb 2>&1 | Out-Null
    if ($LASTEXITCODE -eq 0) {
      Ok "Codex: MCP server 'hellodb' already configured"
    } else {
      & codex mcp add hellodb -- $McpExe 2>&1 | Out-Null
      if ($LASTEXITCODE -eq 0) {
        Ok "Codex: registered stdio MCP → $McpExe"
      } else {
        Warn "Codex: 'codex mcp add' failed — run manually:"
        Warn "  codex mcp add hellodb -- $McpExe"
      }
    }
  }

} finally {
  if (Test-Path $Tmp) { Remove-Item -Recurse -Force $Tmp }
}

Write-Host ""
Ok "done."
Write-Host ""
Write-Host "next:"
Write-Host "  1. open a NEW PowerShell window so the updated PATH is visible"
Write-Host "  2. restart Claude Code to pick up the plugin (if you use it)"
Write-Host "  3. Codex: MCP auto-registered if 'codex' was on PATH; else: hellodb integrate codex"
Write-Host "  4. optional Cloudflare embeddings:"
Write-Host "       git clone https://github.com/$Repo"
Write-Host "       cd hellodb"
Write-Host "       make setup-cloudflare     # requires make + bash (Git for Windows provides both)"
Write-Host ""
Write-Host "docs: https://github.com/$Repo"
