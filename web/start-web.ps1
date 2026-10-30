[CmdletBinding(PositionalBinding = $false)]
param(
    [switch]$NoOpen,
    [switch]$NoBuild,
    [ValidateSet('builtin', 'off')]
    [string]$AuthMode,
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$Command = @()
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$webRoot = $PSScriptRoot
$executable = Join-Path $webRoot 'target\release\speeching-web.exe'
$manifest = Join-Path $webRoot 'Cargo.toml'

function Test-ReleaseCurrent {
    if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) { return $false }
    $builtAt = (Get-Item -LiteralPath $executable).LastWriteTimeUtc
    # Files used only by tests do not change the release binary.
    $inputs = @(
        Get-Item -LiteralPath $manifest
        Get-Item -LiteralPath (Join-Path $webRoot 'Cargo.lock')
        Get-ChildItem -LiteralPath (Join-Path $webRoot 'src') -File -Recurse | Where-Object { $_.Name -notmatch '(^tests|_tests)\.rs$' }
        Get-ChildItem -LiteralPath (Join-Path $webRoot 'static') -File -Recurse
    )
    return -not ($inputs | Where-Object { $_.LastWriteTimeUtc -gt $builtAt } | Select-Object -First 1)
}

function Prepare-Release {
    if ($NoBuild -or (Test-ReleaseCurrent)) {
        if (Test-Path -LiteralPath $executable -PathType Leaf) { return }
        throw 'The web executable is missing. Install Rust and run start-web.cmd without -NoBuild.'
    }
    $cargo = Get-Command cargo -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $cargo) {
        throw 'The web executable is missing or older than its source. Install Rust and Windows build tools, then run start-web.cmd again.'
    }
    Write-Host 'Building the current Rust web release...'
    $previousTarget = $env:CARGO_TARGET_DIR
    try {
        $env:CARGO_TARGET_DIR = Join-Path $webRoot 'target'
        & $cargo.Source build --manifest-path $manifest --release --locked
        if ($LASTEXITCODE -ne 0) { throw 'The web release could not be built. See the errors above.' }
    } finally {
        $env:CARGO_TARGET_DIR = $previousTarget
    }
    if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
        throw 'The build finished without producing speeching-web.exe.'
    }
}

function Get-PrivateSecurity([bool]$Directory) {
    $security = if ($Directory) {
        New-Object System.Security.AccessControl.DirectorySecurity
    } else {
        New-Object System.Security.AccessControl.FileSecurity
    }
    $security.SetAccessRuleProtection($true, $false)
    $currentUser = [System.Security.Principal.WindowsIdentity]::GetCurrent().User
    $security.SetOwner($currentUser)
    $identities = @($currentUser.Value, 'S-1-5-18', 'S-1-5-32-544') | Select-Object -Unique
    foreach ($identity in $identities) {
        $sid = New-Object System.Security.Principal.SecurityIdentifier($identity)
        if ($Directory) {
            $inheritance = [System.Security.AccessControl.InheritanceFlags]::ContainerInherit -bor
                [System.Security.AccessControl.InheritanceFlags]::ObjectInherit
            $rule = New-Object System.Security.AccessControl.FileSystemAccessRule(
                $sid, 'FullControl', $inheritance, 'None', 'Allow')
        } else {
            $rule = New-Object System.Security.AccessControl.FileSystemAccessRule($sid, 'FullControl', 'Allow')
        }
        $security.AddAccessRule($rule)
    }
    return $security
}

function Ensure-PrivateDirectory([string]$Path) {
    if (Test-Path -LiteralPath $Path -PathType Container) { return }
    $security = Get-PrivateSecurity $true
    $null = [System.IO.Directory]::CreateDirectory($Path, $security)
}

function Resolve-WebPath([string]$Path) {
    if ([System.IO.Path]::IsPathRooted($Path)) { return [System.IO.Path]::GetFullPath($Path) }
    return [System.IO.Path]::GetFullPath((Join-Path $webRoot $Path))
}

function Get-DefaultDataDirectory {
    if ([string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
        throw 'LOCALAPPDATA is unavailable. Set SPEECHING_WEB_DATA to a private directory.'
    }
    $locations = @(
        (Join-Path $env:LOCALAPPDATA 'SpeechingWeb')
        (Join-Path $webRoot 'speeching-web-data')
        (Join-Path (Split-Path -Parent $webRoot) 'speeching-web-data')
    )
    $existing = @($locations | Where-Object { Test-Path -LiteralPath (Join-Path $_ 'speeching-web.sqlite3') -PathType Leaf })
    if ($existing.Count -gt 1) {
        throw 'Multiple web databases were found. Set SPEECHING_WEB_DATA to the one you want to use.'
    }
    if ($existing.Count -eq 1) { return $existing[0] }
    return $locations[0]
}

function New-PrivateMasterKey([string]$Path) {
    Ensure-PrivateDirectory ([System.IO.Path]::GetDirectoryName($Path))
    $stream = $null
    $created = $false
    $failure = $null
    $bytes = New-Object byte[] 32
    try {
        $random = [System.Security.Cryptography.RandomNumberGenerator]::Create()
        try { $random.GetBytes($bytes) } finally { $random.Dispose() }
        $encoded = [Convert]::ToBase64String($bytes).TrimEnd('=').Replace('+', '-').Replace('/', '_')
        $security = Get-PrivateSecurity $false
        $stream = New-Object System.IO.FileStream(
            $Path, [System.IO.FileMode]::CreateNew, [System.Security.AccessControl.FileSystemRights]::Write,
            [System.IO.FileShare]::None, 4096, [System.IO.FileOptions]::None, $security)
        $created = $true
        $content = [System.Text.Encoding]::ASCII.GetBytes($encoded)
        $stream.Write($content, 0, $content.Length)
        $stream.Flush($true)
    } catch {
        $failure = $_
    } finally {
        if ($stream) { $stream.Dispose() }
        [Array]::Clear($bytes, 0, $bytes.Length)
    }
    if ($failure) {
        if ($created) { Remove-Item -LiteralPath $Path -Force -ErrorAction SilentlyContinue }
        throw "Could not securely create the web master key. $failure"
    }
}

function Set-LocalOrigin {
    if ($env:SPEECHING_ORIGIN -or -not $env:SPEECHING_LISTEN) { return }
    try { $uri = [Uri]("http://{0}/" -f $env:SPEECHING_LISTEN) } catch {
        throw 'Set SPEECHING_ORIGIN to the browser origin for this listen address.'
    }
    if ($uri.Host -notin @('127.0.0.1', '[::1]', '::1') -or $uri.Port -eq 0) {
        throw 'Set SPEECHING_ORIGIN to the browser origin for this listen address.'
    }
    $env:SPEECHING_ORIGIN = $uri.GetLeftPart([System.UriPartial]::Authority)
}

function Initialize-WebEnvironment {
    Set-LocalOrigin
    if ([string]::IsNullOrWhiteSpace($env:SPEECHING_WEB_DATA)) {
        $env:SPEECHING_WEB_DATA = Get-DefaultDataDirectory
    }
    $env:SPEECHING_WEB_DATA = Resolve-WebPath $env:SPEECHING_WEB_DATA
    $database = Join-Path $env:SPEECHING_WEB_DATA 'speeching-web.sqlite3'
    if (-not $env:SPEECHING_MASTER_KEY_FILE -and -not $env:SPEECHING_MASTER_KEY) {
        if ([string]::IsNullOrWhiteSpace($env:USERPROFILE)) { throw 'USERPROFILE is unavailable. Set SPEECHING_MASTER_KEY_FILE to a protected key file.' }
        $env:SPEECHING_MASTER_KEY_FILE = Join-Path $env:USERPROFILE '.config\speeching-web\master.key'
    }
    if ($env:SPEECHING_MASTER_KEY_FILE) {
        $env:SPEECHING_MASTER_KEY_FILE = Resolve-WebPath $env:SPEECHING_MASTER_KEY_FILE
        if (-not (Test-Path -LiteralPath $env:SPEECHING_MASTER_KEY_FILE -PathType Leaf)) {
            if ((Test-Path -LiteralPath $database) -or (Test-Path -LiteralPath "$database-wal")) {
                throw 'The web database already exists but its master key is missing. Restore the original key; a replacement would make saved API keys unreadable.'
            }
            New-PrivateMasterKey $env:SPEECHING_MASTER_KEY_FILE
            Write-Host 'Created a protected web master key. Back it up securely; it is needed to recover saved provider keys.'
        }
    }
    Ensure-PrivateDirectory $env:SPEECHING_WEB_DATA
    return $database
}

function Invoke-AccountCommand([string[]]$Arguments) {
    if ($Arguments.Count -ne 3 -or $Arguments[0] -ne 'user' -or $Arguments[1] -ne 'add') {
        throw 'Usage: start-web.cmd [-NoOpen] [-NoBuild] [user add USERNAME]'
    }
    & $executable @Arguments
    if ($LASTEXITCODE -ne 0) { throw 'The account was not saved.' }
}

function Initialize-Account([string]$Database) {
    $mode = if ($env:SPEECHING_AUTH) { $env:SPEECHING_AUTH } else { 'builtin' }
    if ($mode -ne 'builtin' -or $env:SPEECHING_ALLOW_REGISTRATION -ne 'false' -or (Test-Path -LiteralPath $Database)) { return }
    Write-Host 'Create the first web account. The password is not displayed while you type.'
    $username = Read-Host 'Username'
    if ([string]::IsNullOrWhiteSpace($username)) { throw 'Account setup canceled. Run start-web.cmd again to create an account.' }
    Invoke-AccountCommand @('user', 'add', $username)
}

function Normalize-AuthenticationChoice([string]$Value) {
    $choice = $Value.Trim()
    $prefix = [Console]::InputEncoding.GetString([byte[]](0xEF, 0xBB, 0xBF))
    if ($prefix -match '[^\x00-\x7F]' -and $choice.StartsWith($prefix, [StringComparison]::Ordinal)) {
        $choice = $choice.Substring($prefix.Length).Trim()
    }
    return $choice.Trim([char]0xFEFF).Trim()
}

function Initialize-Authentication {
    $setting = Join-Path $env:SPEECHING_WEB_DATA 'launcher-auth'
    if ($AuthMode) {
        $env:SPEECHING_AUTH = $AuthMode
        [IO.File]::WriteAllText($setting, $AuthMode)
    } elseif ($env:SPEECHING_AUTH) {
        return
    } elseif (Test-Path -LiteralPath $setting -PathType Leaf) {
        $env:SPEECHING_AUTH = [IO.File]::ReadAllText($setting).Trim()
    } else {
        Write-Host 'Choose how people access Speeching web:'
        Write-Host '  1) Built-in login and separate accounts (recommended)'
        Write-Host '  2) No login (one shared guest; protect access externally)'
        $choice = Normalize-AuthenticationChoice ([string](Read-Host 'Choice [1]'))
        switch ($choice) {
            { $_ -eq '' -or $_ -eq '1' } { $env:SPEECHING_AUTH = 'builtin' }
            '2' { $env:SPEECHING_AUTH = 'off' }
            default { throw 'Choose 1 or 2 and restart.' }
        }
        [IO.File]::WriteAllText($setting, $env:SPEECHING_AUTH)
    }
    if ($env:SPEECHING_AUTH -notin @('builtin', 'off')) {
        throw 'The saved launcher authentication setting is invalid.'
    }
}

function Open-PracticeTab([string]$Url) {
    $browser = $env:SPEECHING_WEB_BROWSER
    if ($browser -and -not (Test-Path -LiteralPath $browser -PathType Leaf)) {
        Write-Host "The configured browser was not found. Open $Url manually."
        return
    }
    $masterKey = $env:SPEECHING_MASTER_KEY
    $proxySecret = $env:SPEECHING_PROXY_SECRET
    try {
        $env:SPEECHING_MASTER_KEY = $null
        $env:SPEECHING_PROXY_SECRET = $null
        if ($browser) {
            Start-Process -FilePath $browser -ArgumentList @($Url) | Out-Null
        } else {
            Start-Process -FilePath $Url | Out-Null
        }
    } catch {
        Write-Host "Could not open the browser. Open $Url manually."
    } finally {
        $env:SPEECHING_MASTER_KEY = $masterKey
        $env:SPEECHING_PROXY_SECRET = $proxySecret
    }
}

function Start-WebService {
    $url = if ($env:SPEECHING_ORIGIN) { $env:SPEECHING_ORIGIN.TrimEnd('/') } else { 'http://127.0.0.1:8787' }
    Write-Host "Starting Speeching web. Open $url if a browser does not appear."
    Write-Host 'Keep this window open; press Ctrl+C to stop the web service.'
    $opened = $false
    $previousPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        & $executable 2>&1 | ForEach-Object {
            Write-Host $_
            if (-not $NoOpen -and -not $opened -and "$_".Contains('Speeching web started')) {
                $opened = $true
                Open-PracticeTab "$url/"
            }
        }
        if ($LASTEXITCODE -ne 0) { throw "The web service exited with code $LASTEXITCODE." }
    } finally {
        $ErrorActionPreference = $previousPreference
    }
}

try {
    Push-Location -LiteralPath $webRoot
    Prepare-Release
    $database = Initialize-WebEnvironment
    if ($Command.Count -gt 0) {
        Invoke-AccountCommand $Command
    } else {
        Initialize-Authentication
        Initialize-Account $database
        Start-WebService
    }
} catch {
    Write-Host "Speeching web: $_" -ForegroundColor Red
    exit 1
} finally {
    Pop-Location
}
