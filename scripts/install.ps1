# Installs zygo from a release: no administrator rights, no profile or registry edit.
# Usage: install.ps1 [-Version <v>] [-DryRun]
# Environment: ZYGO_INSTALL_DIR (default %LOCALAPPDATA%\zygo\bin), ZYGO_BASE_URL (mirror), ZYGO_TARGET (target triple),
# ZYGO_ACT_LITE_INSTALLER (URL of act-lite's install.ps1).
# Code navigation: act101's `act` found -> plugin act101:act101; else `act-lite` found -> act101:act-lite;
# else act-lite is installed by its own installer and act101:act-lite is used.
param(
    [string]$Version = '',
    [switch]$DryRun
)
$Repo = 'act101-ai/zygo'
# Under `irm ... | iex` this runs in the user's own PowerShell host, so no path may end the host:
# a failure writes its message and throws (a script run with -File then exits 1).
function Fail([string]$Message) {
    [Console]::Error.WriteLine("install: $Message")
    throw "install: $Message"
}

# A file:// base is read from disk (PowerShell 7 Invoke-WebRequest does not support file://).
function Get-Remote([string]$Url, [string]$OutFile) {
    try {
        if ($Url.StartsWith('file://')) {
            $path = ([System.Uri]$Url).LocalPath
            [System.IO.File]::WriteAllBytes($OutFile, [System.IO.File]::ReadAllBytes($path))
        } else {
            Invoke-WebRequest -Uri $Url -OutFile $OutFile -UseBasicParsing | Out-Null
        }
    } catch {
        Fail "cannot download ${Url}: $($_.Exception.Message)"
    }
}

function Install-Zygo {
    $ErrorActionPreference = 'Stop'

    if ($Version -ne '') {
        if ($Version -notmatch '^v?[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$') {
            Fail "bad version $Version"
        }
        $Version = $Version -replace '^v', ''
    }

    if ($env:ZYGO_TARGET) {
        $target = $env:ZYGO_TARGET
    } else {
        $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
        switch ($arch) {
            'X64' { $target = 'x86_64-pc-windows-msvc' }
            'Arm64' { $target = 'aarch64-pc-windows-msvc' }
            default { Fail "unsupported platform windows/$arch" }
        }
    }

    $name = "zygo-$target.exe"
    $base = if ($env:ZYGO_BASE_URL) { $env:ZYGO_BASE_URL } else { "https://github.com/$Repo/releases" }
    if ($Version -ne '') {
        $urlDir = "$base/download/v$Version"
        $label = $Version
    } else {
        $urlDir = "$base/latest/download"
        $label = 'latest'
    }
    $dir = if ($env:ZYGO_INSTALL_DIR) { $env:ZYGO_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'zygo\bin' }
    $dest = Join-Path $dir 'zygo.exe'
    $actLiteInstaller = if ($env:ZYGO_ACT_LITE_INSTALLER) { $env:ZYGO_ACT_LITE_INSTALLER } else { 'https://raw.githubusercontent.com/act101-ai/act-lite/main/install.ps1' }

    # act101's act prints `act <version>`; nektos/act (GitHub Actions runner) prints `act version <version>`.
    $nav = 'none'
    $act = Get-Command act -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($act) {
        try { $line = & $act.Source --version 2>$null | Select-Object -First 1 } catch { $line = '' }
        if ("$line" -match '^act [0-9]') { $nav = 'act' }
    }
    if ($nav -eq 'none' -and (Get-Command act-lite -CommandType Application -ErrorAction SilentlyContinue)) { $nav = 'act-lite' }

    if ($DryRun) {
        Write-Output "target: $target"
        Write-Output "would download $urlDir/$name"
        Write-Output "would download $urlDir/SHA256SUMS"
        Write-Output 'would verify checksum'
        Write-Output "would install $dest"
        switch ($nav) {
            'act' { Write-Output 'found act: would enable act101:act101 and disable act101:act-lite' }
            'act-lite' { Write-Output 'found act-lite: would enable act101:act-lite and disable act101:act101' }
            default {
                Write-Output "would install act-lite with $actLiteInstaller"
                Write-Output 'would enable act101:act-lite and disable act101:act101'
            }
        }
        return
    }

    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $tmp = Join-Path $dir ".zygo-install.$PID.exe"
    $sums = Join-Path $dir ".zygo-install.$PID.sums"
    $alps = Join-Path $dir ".zygo-install.$PID.act-lite.ps1"
    try {
        Get-Remote "$urlDir/SHA256SUMS" $sums
        $expected = $null
        foreach ($line in Get-Content -LiteralPath $sums) {
            $parts = $line -split '\s+', 2
            if ($parts.Count -lt 2) { continue }
            if ($parts[1].TrimStart('*').Trim() -eq $name) { $expected = $parts[0].ToLowerInvariant(); break }
        }
        if (-not $expected) { Fail "no checksum for $name" }
        Get-Remote "$urlDir/$name" $tmp
        $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $tmp).Hash.ToLowerInvariant()
        if ($actual -ne $expected) { Fail "checksum mismatch for $name" }

        $same = $false
        if (Test-Path -LiteralPath $dest) {
            $same = ((Get-FileHash -Algorithm SHA256 -LiteralPath $dest).Hash.ToLowerInvariant() -eq $actual)
        }
        if ($same) {
            Write-Output "zygo $label already installed"
        } else {
            Move-Item -Force -LiteralPath $tmp -Destination $dest
            Write-Output "installed zygo $label to $dest"
        }

        # act-lite's installer runs in a child process, so nothing it does can end this host.
        if ($nav -eq 'none') {
            Get-Remote $actLiteInstaller $alps
            $hostExe = (Get-Process -Id $PID).Path
            & $hostExe -NoProfile -ExecutionPolicy Bypass -File $alps
            if ($LASTEXITCODE -ne 0) { Fail 'act-lite install failed; see https://github.com/act101-ai/act-lite#install' }
            $nav = 'act-lite'
        }
        # The plugin selection is written with the zygo just installed (spec §15.2), with empty input.
        $use = if ($nav -eq 'act') { @('act101:act101', 'act101:act-lite') } else { @('act101:act-lite', 'act101:act101') }
        foreach ($step in @(@('enable', $use[0], '--yes'), @('disable', $use[1]))) {
            $null | & $dest plugins @step | Out-Null
            if ($LASTEXITCODE -ne 0) { Fail "zygo plugins $($step -join ' ') failed; run it again when the cause is fixed" }
        }
        if ($nav -eq 'act') { Write-Output 'found act: using plugin act101:act101' } else { Write-Output 'using plugin act101:act-lite' }
    } finally {
        Remove-Item -Force -ErrorAction SilentlyContinue -LiteralPath $tmp, $sums, $alps
    }

    if (-not (($env:PATH -split ';') -contains $dir)) { Write-Output "add $dir to PATH" }
}

Install-Zygo
