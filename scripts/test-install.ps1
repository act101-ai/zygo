# Tests for scripts/install.ps1 (run on the Windows CI job): file:// and http:// release fixtures.
$ErrorActionPreference = 'Stop'
$Script = Join-Path $PSScriptRoot 'install.ps1'
$T = Join-Path ([System.IO.Path]::GetTempPath()) ("zygo-install-test-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $T | Out-Null
$server = $null
$exe = (Get-Process -Id $PID).Path

$arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
$Triple = if ($arch -eq 'Arm64') { 'aarch64-pc-windows-msvc' } else { 'x86_64-pc-windows-msvc' }
$Name = "zygo-$Triple.exe"
$Case = ''

function Fail([string]$Message) {
    [Console]::Error.WriteLine("test-install: FAIL ${Case}: $Message")
    exit 1
}

function New-Release([string]$Dir, [string]$Content, [string]$Sums = '') {
    New-Item -ItemType Directory -Force -Path $Dir | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $Dir $Name), $Content)
    if ($Sums -eq '') {
        $h = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $Dir $Name)).Hash.ToLowerInvariant()
        $Sums = "$h  $Name`n"
    }
    [System.IO.File]::WriteAllText((Join-Path $Dir 'SHA256SUMS'), $Sums)
}

# Runs install.ps1 in a child process; the environment goes on the child only.
function Invoke-Install([string]$Base, [string]$InstallDir, [string[]]$ScriptArgs = @()) {
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $exe
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.UseShellExecute = $false
    foreach ($a in @('-NoProfile', '-File', $Script) + $ScriptArgs) { $psi.ArgumentList.Add($a) }
    $psi.Environment['ZYGO_BASE_URL'] = $Base
    $psi.Environment['ZYGO_INSTALL_DIR'] = $InstallDir
    $psi.Environment.Remove('ZYGO_TARGET') | Out-Null
    $p = [System.Diagnostics.Process]::Start($psi)
    $out = $p.StandardOutput.ReadToEnd() + $p.StandardError.ReadToEnd()
    $p.WaitForExit()
    return [pscustomobject]@{ Code = $p.ExitCode; Out = $out }
}

function Reset-Fixtures {
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue (Join-Path $T 'rel')
    New-Release (Join-Path $T 'rel\latest\download') 'fixture-binary'
    New-Release (Join-Path $T 'rel\download\v1.2.3') 'tagged-binary'
}

try {
    $base = 'file:///' + ((Join-Path $T 'rel') -replace '\\', '/')
    Reset-Fixtures

    $Case = 'dry_run'
    $d = Join-Path $T 'bin1'
    $r = Invoke-Install $base $d @('-DryRun')
    if ($r.Code -ne 0) { Fail "exit $($r.Code)" }
    if ($r.Out -notmatch "target: $Triple") { Fail "no target line: $($r.Out)" }
    if (Test-Path $d) { Fail 'dry run created the directory' }

    $Case = 'install_over_file'
    $d = Join-Path $T 'bin2'
    $r = Invoke-Install $base $d
    if ($r.Code -ne 0) { Fail "exit $($r.Code): $($r.Out)" }
    if ([System.IO.File]::ReadAllText((Join-Path $d 'zygo.exe')) -ne 'fixture-binary') { Fail 'wrong bytes' }
    if ($r.Out -notmatch 'installed zygo') { Fail $r.Out }

    $Case = 'idempotent'
    $before = Get-Item (Join-Path $d 'zygo.exe')
    $r = Invoke-Install $base $d
    if ($r.Code -ne 0 -or $r.Out -notmatch 'already installed') { Fail $r.Out }
    $after = Get-Item (Join-Path $d 'zygo.exe')
    if ($before.CreationTimeUtc -ne $after.CreationTimeUtc) { Fail 'file was rewritten' }

    $Case = 'checksum_mismatch'
    $zeros = '0' * 64
    New-Release (Join-Path $T 'rel\latest\download') 'fixture-binary' "$zeros  $Name`n"
    $d = Join-Path $T 'bin3'
    $r = Invoke-Install $base $d
    if ($r.Code -eq 0 -or $r.Out -notmatch 'checksum mismatch') { Fail $r.Out }
    if (Test-Path (Join-Path $d 'zygo.exe')) { Fail 'binary was installed' }

    $Case = 'version_flag'
    Reset-Fixtures
    $d = Join-Path $T 'bin4'
    $r = Invoke-Install $base $d @('-Version', 'v1.2.3')
    if ($r.Code -ne 0) { Fail $r.Out }
    if ([System.IO.File]::ReadAllText((Join-Path $d 'zygo.exe')) -ne 'tagged-binary') { Fail 'wrong bytes' }

    $Case = 'no_checksum_line'
    New-Release (Join-Path $T 'rel\latest\download') 'fixture-binary' "$zeros  other-file`n"
    $d = Join-Path $T 'bin5'
    $r = Invoke-Install $base $d
    if ($r.Code -eq 0 -or $r.Out -notmatch 'no checksum for') { Fail $r.Out }
    if (Test-Path (Join-Path $d 'zygo.exe')) { Fail 'binary was installed' }

    $Case = 'bad_version'
    $d = Join-Path $T 'bin6'
    $r = Invoke-Install $base $d @('-Version', 'abc')
    if ($r.Code -eq 0 -or $r.Out -notmatch 'bad version abc') { Fail "exit $($r.Code): $($r.Out)" }
    if (Test-Path $d) { Fail 'created the directory' }

    # Under `irm ... | iex` an `exit` closes the user's PowerShell host, so install.ps1 fails by `throw` only.
    $Case = 'no_exit'
    $code = (Get-Content -LiteralPath $Script) | Where-Object { $_ -notmatch '^\s*#' }
    $hits = $code | Where-Object { $_ -match '\bexit\b' }
    if ($hits) { Fail "install.ps1 calls exit: $hits" }

    $Case = 'install_over_http'
    Reset-Fixtures
    $port = Get-Random -Minimum 20000 -Maximum 40000
    $server = Start-Process -FilePath python -ArgumentList @('-m', 'http.server', "$port", '--bind', '127.0.0.1', '--directory', (Join-Path $T 'rel')) -PassThru -WindowStyle Hidden
    $ok = $false
    for ($i = 0; $i -lt 50; $i++) {
        try { Invoke-WebRequest -Uri "http://127.0.0.1:$port/latest/download/SHA256SUMS" -UseBasicParsing | Out-Null; $ok = $true; break } catch { Start-Sleep -Milliseconds 200 }
    }
    if (-not $ok) { Fail 'http server did not start' }
    $d = Join-Path $T 'bin7'
    $r = Invoke-Install "http://127.0.0.1:$port" $d
    if ($r.Code -ne 0) { Fail $r.Out }
    if ([System.IO.File]::ReadAllText((Join-Path $d 'zygo.exe')) -ne 'fixture-binary') { Fail 'wrong bytes' }

    Write-Output 'test-install: all passed'
} finally {
    if ($server) { Stop-Process -Id $server.Id -Force -ErrorAction SilentlyContinue }
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $T
}
