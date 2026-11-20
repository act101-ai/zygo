# Installs zygo from a release: no administrator rights, no profile or registry edit.
# Usage: install.ps1 [-Version <v>] [-DryRun]
# Environment: ZYGO_INSTALL_DIR (default %LOCALAPPDATA%\zygo\bin), ZYGO_BASE_URL (mirror), ZYGO_TARGET (target triple).
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

    if ($DryRun) {
        Write-Output "target: $target"
        Write-Output "would download $urlDir/$name"
        Write-Output "would download $urlDir/SHA256SUMS"
        Write-Output 'would verify checksum'
        Write-Output "would install $dest"
        return
    }

    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $tmp = Join-Path $dir ".zygo-install.$PID.exe"
    $sums = Join-Path $dir ".zygo-install.$PID.sums"
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
    } finally {
        Remove-Item -Force -ErrorAction SilentlyContinue -LiteralPath $tmp, $sums
    }

    if (-not (($env:PATH -split ';') -contains $dir)) { Write-Output "add $dir to PATH" }
}

Install-Zygo
