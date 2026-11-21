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

# A stub zygo.exe: logs its arguments to ZYGO_STUB_LOG and exits 1 when ZYGO_STUB_FAIL is set. $Marker makes the
# two fixture builds differ.
function New-Stub([string]$Marker) {
    $csc = Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
    if (-not (Test-Path $csc)) { Fail "no $csc" }
    $src = Join-Path $T "$Marker.cs"
    $out = Join-Path $T "$Marker.exe"
    [System.IO.File]::WriteAllText($src, @"
using System; using System.IO;
class P { const string M = "$Marker"; static int Main(string[] a) {
    string l = Environment.GetEnvironmentVariable("ZYGO_STUB_LOG");
    if (l != null) File.AppendAllText(l, "zygo " + string.Join(" ", a) + "\n");
    return Environment.GetEnvironmentVariable("ZYGO_STUB_FAIL") != null ? 1 : 0; } }
"@)
    & $csc /nologo /out:$out $src | Out-Null
    if ($LASTEXITCODE -ne 0) { Fail "csc failed for $Marker" }
    return $out
}

# Puts stubs of act101's act, act-lite or nektos/act (named nektos) in the child's PATH, and nothing else.
function Set-Acts([string[]]$Names = @()) {
    $d = Join-Path $T 'acts'
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $d
    New-Item -ItemType Directory -Path $d | Out-Null
    foreach ($n in $Names) {
        switch ($n) {
            'act' { Set-Content -LiteralPath (Join-Path $d 'act.cmd') -Value '@echo act 2.9.2' }
            'nektos' { Set-Content -LiteralPath (Join-Path $d 'act.cmd') -Value '@echo act version 0.2.61' }
            'act-lite' { Set-Content -LiteralPath (Join-Path $d 'act-lite.cmd') -Value '@echo act-lite 1.2.7' }
        }
    }
}

function Get-Log {
    $f = Join-Path $T 'log'
    if (-not (Test-Path $f)) { return '' }
    return ((Get-Content -LiteralPath $f) -join '|')
}

function Same([string]$A, [string]$B) {
    (Get-FileHash -Algorithm SHA256 -LiteralPath $A).Hash -eq (Get-FileHash -Algorithm SHA256 -LiteralPath $B).Hash
}

function New-Release([string]$Dir, [string]$Exe, [string]$Sums = '') {
    New-Item -ItemType Directory -Force -Path $Dir | Out-Null
    Copy-Item -Force -LiteralPath $Exe -Destination (Join-Path $Dir $Name)
    if ($Sums -eq '') {
        $h = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $Dir $Name)).Hash.ToLowerInvariant()
        $Sums = "$h  $Name`n"
    }
    [System.IO.File]::WriteAllText((Join-Path $Dir 'SHA256SUMS'), $Sums)
}

# Runs install.ps1 in a child process; the environment goes on the child only.
function Invoke-Install([string]$Base, [string]$InstallDir, [string[]]$ScriptArgs = @(), [hashtable]$Env = @{}) {
    Remove-Item -Force -ErrorAction SilentlyContinue (Join-Path $T 'log')
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $exe
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.UseShellExecute = $false
    foreach ($a in @('-NoProfile', '-File', $Script) + $ScriptArgs) { $psi.ArgumentList.Add($a) }
    $psi.Environment['ZYGO_BASE_URL'] = $Base
    $psi.Environment['ZYGO_INSTALL_DIR'] = $InstallDir
    $psi.Environment.Remove('ZYGO_TARGET') | Out-Null
    $psi.Environment['ZYGO_ACT_LITE_INSTALLER'] = 'file:///' + ((Join-Path $T 'al\install.ps1') -replace '\\', '/')
    $psi.Environment['ZYGO_STUB_LOG'] = Join-Path $T 'log'
    $psi.Environment['PATH'] = (Join-Path $T 'acts') + ";$env:SystemRoot\System32"
    foreach ($k in $Env.Keys) { $psi.Environment[$k] = $Env[$k] }
    $p = [System.Diagnostics.Process]::Start($psi)
    $out = $p.StandardOutput.ReadToEnd() + $p.StandardError.ReadToEnd()
    $p.WaitForExit()
    return [pscustomobject]@{ Code = $p.ExitCode; Out = $out }
}

function Reset-Fixtures {
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue (Join-Path $T 'rel')
    New-Release (Join-Path $T 'rel\latest\download') $FixtureExe
    New-Release (Join-Path $T 'rel\download\v1.2.3') $TaggedExe
}

try {
    $FixtureExe = New-Stub 'fixture'
    $TaggedExe = New-Stub 'tagged'
    New-Item -ItemType Directory -Path (Join-Path $T 'al') | Out-Null
    Set-Content -LiteralPath (Join-Path $T 'al\install.ps1') -Value @(
        "Add-Content -LiteralPath `$env:ZYGO_STUB_LOG -Value 'act-lite-installer'",
        'if ($env:AL_FAIL) { exit 1 }')
    Set-Acts
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
    if (-not (Same (Join-Path $d 'zygo.exe') $FixtureExe)) { Fail 'wrong bytes' }
    if ($r.Out -notmatch 'installed zygo') { Fail $r.Out }
    if ((Get-Log) -ne 'act-lite-installer|zygo plugins enable act101:act-lite --yes|zygo plugins disable act101:act101') { Fail "log: $(Get-Log)" }

    $Case = 'idempotent'
    $before = Get-Item (Join-Path $d 'zygo.exe')
    $r = Invoke-Install $base $d
    if ($r.Code -ne 0 -or $r.Out -notmatch 'already installed') { Fail $r.Out }
    $after = Get-Item (Join-Path $d 'zygo.exe')
    if ($before.CreationTimeUtc -ne $after.CreationTimeUtc) { Fail 'file was rewritten' }

    $Case = 'checksum_mismatch'
    $zeros = '0' * 64
    New-Release (Join-Path $T 'rel\latest\download') $FixtureExe "$zeros  $Name`n"
    $d = Join-Path $T 'bin3'
    $r = Invoke-Install $base $d
    if ($r.Code -eq 0 -or $r.Out -notmatch 'checksum mismatch') { Fail $r.Out }
    if (Test-Path (Join-Path $d 'zygo.exe')) { Fail 'binary was installed' }

    $Case = 'version_flag'
    Reset-Fixtures
    $d = Join-Path $T 'bin4'
    $r = Invoke-Install $base $d @('-Version', 'v1.2.3')
    if ($r.Code -ne 0) { Fail $r.Out }
    if (-not (Same (Join-Path $d 'zygo.exe') $TaggedExe)) { Fail 'wrong bytes' }

    $Case = 'no_checksum_line'
    New-Release (Join-Path $T 'rel\latest\download') $FixtureExe "$zeros  other-file`n"
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
    if (-not (Same (Join-Path $d 'zygo.exe') $FixtureExe)) { Fail 'wrong bytes' }
    Stop-Process -Id $server.Id -Force -ErrorAction SilentlyContinue
    $server = $null

    $Case = 'act_lite_found_is_used_and_not_reinstalled'
    Reset-Fixtures
    $d = Join-Path $T 'bin8'
    Set-Acts @('act-lite')
    $r = Invoke-Install $base $d
    if ($r.Code -ne 0 -or $r.Out -notmatch 'using plugin act101:act-lite') { Fail $r.Out }
    if ((Get-Log) -ne 'zygo plugins enable act101:act-lite --yes|zygo plugins disable act101:act101') { Fail "log: $(Get-Log)" }

    $Case = 'act_found_selects_the_act101_plugin'
    Set-Acts @('act', 'act-lite')
    $r = Invoke-Install $base $d
    if ($r.Code -ne 0 -or $r.Out -notmatch 'found act: using plugin act101:act101') { Fail $r.Out }
    if ((Get-Log) -ne 'zygo plugins enable act101:act101 --yes|zygo plugins disable act101:act-lite') { Fail "log: $(Get-Log)" }

    $Case = 'nektos_act_is_not_act101'
    Set-Acts @('nektos')
    $r = Invoke-Install $base $d
    if ($r.Code -ne 0 -or (Get-Log) -notmatch '^act-lite-installer\|') { Fail "$($r.Out) log: $(Get-Log)" }

    $Case = 'dry_run_reports_the_navigation_choice'
    $d = Join-Path $T 'bin9'
    Set-Acts @('act')
    $r = Invoke-Install $base $d @('-DryRun')
    if ($r.Code -ne 0 -or $r.Out -notmatch 'found act: would enable act101:act101 and disable act101:act-lite') { Fail $r.Out }
    Set-Acts
    $r = Invoke-Install $base $d @('-DryRun')
    if ($r.Code -ne 0 -or $r.Out -notmatch 'would install act-lite with file:///') { Fail $r.Out }
    if ((Get-Log) -ne '' -or (Test-Path $d)) { Fail "dry run ran: $(Get-Log)" }

    $Case = 'plugin_failure_fails'
    $d = Join-Path $T 'bin10'
    Set-Acts @('act')
    $r = Invoke-Install $base $d @() @{ ZYGO_STUB_FAIL = '1' }
    if ($r.Code -eq 0 -or $r.Out -notmatch 'zygo plugins enable act101:act101 --yes failed') { Fail $r.Out }

    $Case = 'act_lite_install_failure_fails'
    $d = Join-Path $T 'bin11'
    Set-Acts
    $r = Invoke-Install $base $d @() @{ AL_FAIL = '1' }
    if ($r.Code -eq 0 -or $r.Out -notmatch 'act-lite install failed') { Fail $r.Out }
    if ((Get-Log) -match 'plugins') { Fail "plugins ran: $(Get-Log)" }
    if (Get-ChildItem -Force $d -Filter '.zygo-install.*') { Fail 'left a temp file' }

    Write-Output 'test-install: all passed'
} finally {
    if ($server) { Stop-Process -Id $server.Id -Force -ErrorAction SilentlyContinue }
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $T
}
