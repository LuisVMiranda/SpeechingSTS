Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$webRoot = Split-Path -Parent $PSScriptRoot
$project = Split-Path -Parent $webRoot
$launcher = Join-Path $project 'start-web.cmd'
$expectedServer = (Resolve-Path -LiteralPath (Join-Path $webRoot 'target\release\speeching-web.exe')).Path
$targetRoot = Join-Path $project 'target'
$null = New-Item -ItemType Directory -Force -Path $targetRoot
$testRoot = Join-Path $targetRoot ("launcher-smoke-" + [Guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Path $testRoot
$children = New-Object 'System.Collections.Generic.List[object]'

function Get-FreePort {
    $listener = New-Object System.Net.Sockets.TcpListener([System.Net.IPAddress]::Loopback, 0)
    try { $listener.Start(); return $listener.LocalEndpoint.Port } finally { $listener.Stop() }
}

function Start-TestLauncher([hashtable]$Settings) {
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = $env:ComSpec
    $targetLauncher = if ($Settings.ContainsKey('Launcher')) { $Settings.Launcher } else { $launcher }
    $info.Arguments = '/d /s /c ""{0}" {1}"' -f $targetLauncher, $Settings.Flags
    $info.WorkingDirectory = $testRoot
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    foreach ($name in @($info.EnvironmentVariables.Keys)) {
        if ($name -like 'SPEECHING_*') { $info.EnvironmentVariables.Remove($name) }
    }
    $info.EnvironmentVariables['SPEECHING_AUTH'] = 'off'
    $info.EnvironmentVariables['SPEECHING_WEB_DATA'] = $Settings.Data
    $info.EnvironmentVariables['SPEECHING_LISTEN'] = "127.0.0.1:$($Settings.Port)"
    if (-not $Settings.ContainsKey('AutoOrigin')) {
        $info.EnvironmentVariables['SPEECHING_ORIGIN'] = "http://127.0.0.1:$($Settings.Port)"
    }
    $info.EnvironmentVariables['SPEECHING_LAUNCHER_NO_PAUSE'] = '1'
    if ($Settings.ContainsKey('MasterKey')) {
        $info.EnvironmentVariables['SPEECHING_MASTER_KEY'] = $Settings.MasterKey
    } else {
        $info.EnvironmentVariables['SPEECHING_MASTER_KEY_FILE'] = $Settings.KeyFile
    }
    if ($Settings.ContainsKey('Browser')) {
        $info.EnvironmentVariables['SPEECHING_WEB_BROWSER'] = $Settings.Browser
        $info.EnvironmentVariables['SPEECHING_TEST_BROWSER_CAPTURE'] = $Settings.Capture
        $info.EnvironmentVariables['SPEECHING_PROXY_SECRET'] = 'synthetic-proxy-secret-for-test-only'
    }
    if ($Settings.ContainsKey('BuildMarker')) {
        $info.EnvironmentVariables['SPEECHING_TEST_BUILD_MARKER'] = $Settings.BuildMarker
        $info.EnvironmentVariables['CARGO_NET_OFFLINE'] = 'true'
    }
    $process = New-Object System.Diagnostics.Process
    $process.StartInfo = $info
    if (-not $process.Start()) { throw 'Could not start the test launcher.' }
    $children.Add([pscustomobject]@{ Process=$process; Port=$Settings.Port })
    return $process
}

function Write-FixtureSource([string]$Path, [string]$Version) {
    $code = @'
fn main() {
    let marker = std::env::var("SPEECHING_TEST_BUILD_MARKER").unwrap();
    let args = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    std::fs::write(marker, format!("VERSION:{}", args)).unwrap();
}
'@.Replace('VERSION', $Version)
    [IO.File]::WriteAllText($Path, $code, [System.Text.Encoding]::ASCII)
}

function Test-BuildAndRefresh {
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { return }
    $fixture = Join-Path $testRoot 'build fixture'
    $fixtureWeb = Join-Path $fixture 'web'
    $null = New-Item -ItemType Directory -Force -Path (Join-Path $fixtureWeb 'src'), (Join-Path $fixtureWeb 'static')
    Copy-Item -LiteralPath $launcher -Destination (Join-Path $fixture 'start-web.cmd')
    Copy-Item -LiteralPath (Join-Path $webRoot 'start-web.ps1') -Destination (Join-Path $fixtureWeb 'start-web.ps1')
    [IO.File]::WriteAllText((Join-Path $fixtureWeb 'Cargo.toml'), "[package]`nname = 'speeching-web'`nversion = '0.0.0'`nedition = '2021'`n", [Text.Encoding]::ASCII)
    [IO.File]::WriteAllText((Join-Path $fixtureWeb 'Cargo.lock'), "version = 4`n[[package]]`nname = 'speeching-web'`nversion = '0.0.0'`n", [Text.Encoding]::ASCII)
    $source = Join-Path $fixtureWeb 'src\main.rs'
    Write-FixtureSource $source 'first'
    $marker = Join-Path $testRoot 'build-marker.txt'
    $options = @{
        Port=(Get-FreePort); Data=(Join-Path $testRoot 'build data'); KeyFile=(Join-Path $testRoot 'build key\master.key');
        Flags='user add demo'; Launcher=(Join-Path $fixture 'start-web.cmd'); BuildMarker=$marker
    }
    $process = Start-TestLauncher $options
    if (-not $process.WaitForExit(60000)) { throw 'The missing-release build did not finish.' }
    if ($process.ExitCode -ne 0) { throw "The missing-release build failed: $($process.StandardOutput.ReadToEnd()) $($process.StandardError.ReadToEnd())" }
    if ([IO.File]::ReadAllText($marker) -ne 'first:user add demo') { throw 'The newly built release was not run.' }
    Write-FixtureSource $source 'second'
    (Get-Item -LiteralPath $source).LastWriteTimeUtc = [DateTime]::UtcNow.AddSeconds(2)
    $process = Start-TestLauncher $options
    if (-not $process.WaitForExit(60000)) { throw 'The stale-release rebuild did not finish.' }
    if ($process.ExitCode -ne 0) { throw "The stale-release rebuild failed: $($process.StandardOutput.ReadToEnd()) $($process.StandardError.ReadToEnd())" }
    if ([IO.File]::ReadAllText($marker) -ne 'second:user add demo') { throw 'The stale release was not rebuilt.' }
}

function Wait-Ready([object]$Process, [int]$Port) {
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    while ([DateTime]::UtcNow -lt $deadline) {
        if ($Process.HasExited) { throw "Launcher exited before startup: $($Process.StandardOutput.ReadToEnd()) $($Process.StandardError.ReadToEnd())" }
        try { return Invoke-RestMethod -Uri "http://127.0.0.1:$Port/api/mode" -TimeoutSec 1 } catch { Start-Sleep -Milliseconds 150 }
    }
    throw 'The local test service did not become ready.'
}

function Test-Descendant([int]$ProcessId, [int]$ParentId) {
    $current = $ProcessId
    for ($depth = 0; $depth -lt 8; $depth++) {
        if ($current -eq $ParentId) { return $true }
        $process = Get-CimInstance Win32_Process -Filter "ProcessId=$current"
        if (-not $process) { return $false }
        $current = [int]$process.ParentProcessId
    }
    return $false
}

function Stop-TestLauncher([object]$Process, [int]$Port) {
    $connection = Get-NetTCPConnection -LocalAddress '127.0.0.1' -LocalPort $Port -State Listen -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($connection) {
        $server = Get-Process -Id $connection.OwningProcess
        if ($server.Path -ne $expectedServer -or -not (Test-Descendant $server.Id $Process.Id)) {
            throw 'Refusing to stop a process that does not belong to this test.'
        }
        Stop-Process -Id $server.Id
    }
    if (-not $Process.WaitForExit(15000)) { throw 'The launcher did not exit after its test server stopped.' }
}

function Assert-PrivateAcl([string]$Path) {
    $acl = Get-Acl -LiteralPath $Path
    if (-not $acl.AreAccessRulesProtected) { throw 'The test path inherited permissions.' }
    $current = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    $allowed = @($current, 'S-1-5-18', 'S-1-5-32-544')
    $rules = $acl.GetAccessRules($true, $true, [System.Security.Principal.SecurityIdentifier])
    foreach ($rule in $rules) {
        if ($rule.IsInherited -or $rule.IdentityReference.Value -notin $allowed) { throw 'Unexpected access on the test path.' }
    }
}

function Test-PersistentKey {
    $port = Get-FreePort
    $data = Join-Path $testRoot 'private data'
    $key = Join-Path $testRoot 'private config\master.key'
    $options = @{ Port=$port; Data=$data; KeyFile=$key; Flags='-NoOpen'; AutoOrigin=$true }
    $process = Start-TestLauncher $options
    if ((Wait-Ready $process $port).auth -ne 'off') { throw 'Unexpected authentication mode.' }
    $contents = [IO.File]::ReadAllText($key)
    if ($contents -cnotmatch '^[A-Za-z0-9_-]{43}$') { throw 'Generated key has the wrong format.' }
    Assert-PrivateAcl $key
    Assert-PrivateAcl $data
    $hash = (Get-FileHash -LiteralPath $key).Hash
    $origin = "http://127.0.0.1:$port"
    $body = @{ key='synthetic-provider-key' } | ConvertTo-Json -Compress
    $null = Invoke-RestMethod -Uri "$origin/api/keys/gemini" -Method Post -Headers @{Origin=$origin} -ContentType 'application/json' -Body $body
    Stop-TestLauncher $process $port
    $options.Port = Get-FreePort
    $process = Start-TestLauncher $options
    $null = Wait-Ready $process $options.Port
    $saved = Invoke-RestMethod -Uri "http://127.0.0.1:$($options.Port)/api/me"
    if (-not $saved.keys.gemini -or (Get-FileHash -LiteralPath $key).Hash -ne $hash) { throw 'The key or saved provider credential did not survive restart.' }
    Stop-TestLauncher $process $options.Port
}

function Test-MissingKeyFailsClosed {
    $port = Get-FreePort
    $data = Join-Path $testRoot 'existing data'
    $null = New-Item -ItemType Directory -Path $data
    $database = Join-Path $data 'speeching-web.sqlite3'
    [IO.File]::WriteAllBytes($database, [byte[]](1,2,3,4))
    $hash = (Get-FileHash -LiteralPath $database).Hash
    $key = Join-Path $testRoot 'missing key\master.key'
    $process = Start-TestLauncher @{ Port=$port; Data=$data; KeyFile=$key; Flags='-NoBuild -NoOpen' }
    if (-not $process.WaitForExit(15000)) { throw 'Missing-key check did not finish.' }
    $output = $process.StandardOutput.ReadToEnd() + $process.StandardError.ReadToEnd()
    if ($process.ExitCode -eq 0 -or $output -notmatch 'master key is missing') { throw 'Missing master key was not reported.' }
    if ((Test-Path -LiteralPath $key) -or (Get-FileHash -LiteralPath $database).Hash -ne $hash) { throw 'The launcher replaced a key or changed existing data.' }
}

function New-BrowserProbe {
    $path = Join-Path $testRoot 'browser-probe.exe'
    $source = @'
using System;
using System.IO;
class BrowserProbe {
    [STAThread] static void Main(string[] args) {
        bool leaked = Environment.GetEnvironmentVariable("SPEECHING_MASTER_KEY") != null || Environment.GetEnvironmentVariable("SPEECHING_PROXY_SECRET") != null;
        File.WriteAllLines(Environment.GetEnvironmentVariable("SPEECHING_TEST_BROWSER_CAPTURE"), new[] { string.Join("|", args), leaked ? "leaked" : "clear" });
    }
}
'@
    Add-Type -TypeDefinition $source -OutputAssembly $path -OutputType WindowsApplication
    return $path
}

function Test-BrowserLaunch {
    $port = Get-FreePort
    $capture = Join-Path $testRoot 'browser-arguments.txt'
    $options = @{
        Port=$port; Data=(Join-Path $testRoot 'browser data'); MasterKey=('A' * 43);
        Flags='-NoBuild'; Browser=(New-BrowserProbe); Capture=$capture
    }
    $process = Start-TestLauncher $options
    $null = Wait-Ready $process $port
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    while (-not (Test-Path -LiteralPath $capture) -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 100 }
    if (-not (Test-Path -LiteralPath $capture)) { throw 'The browser was not launched after the service became ready.' }
    $arguments = Get-Content -LiteralPath $capture
    if ($arguments[0] -notmatch "--app=http://127.0.0.1:$port/" -or $arguments[0] -notmatch '--window-size=460,850' -or $arguments[1] -ne 'clear') {
        throw 'The browser launch arguments or secret isolation are incorrect.'
    }
    Stop-TestLauncher $process $port
}

function Test-PortConflictDoesNotOpenBrowser {
    $listener = New-Object System.Net.Sockets.TcpListener([System.Net.IPAddress]::Loopback, 0)
    try {
        $listener.Start()
        $port = $listener.LocalEndpoint.Port
        $capture = Join-Path $testRoot 'unexpected-browser.txt'
        $options = @{
            Port=$port; Data=(Join-Path $testRoot 'conflict data'); MasterKey=('A' * 43);
            Flags='-NoBuild'; Browser=(Join-Path $testRoot 'browser-probe.exe'); Capture=$capture
        }
        $process = Start-TestLauncher $options
        if (-not $process.WaitForExit(15000)) { throw 'Port-conflict check did not finish.' }
        $output = $process.StandardOutput.ReadToEnd() + $process.StandardError.ReadToEnd()
        if ($process.ExitCode -eq 0 -or $output -notmatch 'Could not listen') { throw 'The port conflict was not reported.' }
        if (Test-Path -LiteralPath $capture) { throw 'The browser opened for a server that did not start.' }
    } finally {
        $listener.Stop()
    }
}

try {
    Test-PersistentKey
    Test-MissingKeyFailsClosed
    Test-BrowserLaunch
    Test-PortConflictDoesNotOpenBrowser
    Test-BuildAndRefresh
    Write-Host 'Launcher checks passed: startup, protected key/data, restart, missing-key recovery, browser launch, port conflicts, and release builds.'
} finally {
    foreach ($child in $children) {
        if ($child.Process.HasExited) { continue }
        try { Stop-TestLauncher $child.Process $child.Port } catch {
            if (-not $child.Process.HasExited) { $child.Process.Kill(); $child.Process.WaitForExit() }
        }
    }
    $resolvedRoot = (Resolve-Path -LiteralPath $targetRoot).Path
    $resolvedTest = (Resolve-Path -LiteralPath $testRoot).Path
    if (-not $resolvedTest.StartsWith($resolvedRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Refusing to clean a test directory outside the project target folder.'
    }
    Remove-Item -LiteralPath $resolvedTest -Recurse -Force
}
