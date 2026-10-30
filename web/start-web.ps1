[CmdletBinding(PositionalBinding = $false)]
param(
    [switch]$NoOpen,
    [switch]$NoBuild,
    [string]$AuthMode,
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$Command = @()
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$webRoot = $PSScriptRoot
$executable = Join-Path $webRoot 'target\release\speeching-web.exe'
$manifest = Join-Path $webRoot 'Cargo.toml'
$translationFile = Join-Path $webRoot 'launcher-i18n.tsv'
$script:messages = @{}
$locationPushed = $false
$previousOutputEncoding = [Console]::OutputEncoding

function Get-LauncherText([string]$Key, [object[]]$Values = @()) {
    $value = $script:messages[$Key]
    if ($null -eq $value) { throw "Missing launcher translation: $Key" }
    for ($index = 0; $index -lt $Values.Count; $index++) {
        $value = $value.Replace("{$index}", [string]$Values[$index])
    }
    return $value
}

function Initialize-Language {
    $selected = $env:SPEECHING_LAUNCHER_LANGUAGE
    if (-not $selected) {
        do {
            $selected = Normalize-AuthenticationChoice ([string](Read-Host 'Language / Idioma [EN/ES/PT] (EN)'))
            if (-not $selected) { $selected = 'en' }
            if ($selected.ToLowerInvariant() -notin @('en', 'es', 'pt')) { Write-Host 'EN / ES / PT' }
        } while ($selected.ToLowerInvariant() -notin @('en', 'es', 'pt'))
    }
    $language = $selected.Trim().ToLowerInvariant()
    if ($language -notin @('en', 'es', 'pt')) { throw 'SPEECHING_LAUNCHER_LANGUAGE: EN / ES / PT' }
    $env:SPEECHING_LAUNCHER_LANGUAGE = $language
    $column = switch ($language) { 'es' { 2 } 'pt' { 3 } default { 1 } }
    foreach ($line in [IO.File]::ReadAllLines($translationFile, [Text.Encoding]::UTF8)) {
        $fields = $line.Split([char]9)
        if ($fields.Count -eq 4) { $script:messages[$fields[0]] = $fields[$column] }
    }
}

function Test-ReleaseCurrent {
    if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) { return $false }
    $builtAt = (Get-Item -LiteralPath $executable).LastWriteTimeUtc
    # Files used only by tests do not change the release binary.
    $inputs = @(
        Get-Item -LiteralPath $manifest
        Get-Item -LiteralPath (Join-Path $webRoot 'Cargo.lock')
        Get-Item -LiteralPath $translationFile
        Get-ChildItem -LiteralPath (Join-Path $webRoot 'src') -File -Recurse | Where-Object { $_.Name -notmatch '(^tests|_tests)\.rs$' }
        Get-ChildItem -LiteralPath (Join-Path $webRoot 'static') -File -Recurse
    )
    return -not ($inputs | Where-Object { $_.LastWriteTimeUtc -gt $builtAt } | Select-Object -First 1)
}

function Prepare-Release {
    if ($NoBuild -or (Test-ReleaseCurrent)) {
        if (Test-Path -LiteralPath $executable -PathType Leaf) { return }
        throw (Get-LauncherText 'no_build_windows')
    }
    $cargo = Get-Command cargo -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $cargo) {
        throw (Get-LauncherText 'no_cargo_windows')
    }
    Write-Host (Get-LauncherText 'building')
    $previousTarget = $env:CARGO_TARGET_DIR
    try {
        $env:CARGO_TARGET_DIR = Join-Path $webRoot 'target'
        & $cargo.Source build --manifest-path $manifest --release --locked
        if ($LASTEXITCODE -ne 0) { throw (Get-LauncherText 'build_failed') }
    } finally {
        $env:CARGO_TARGET_DIR = $previousTarget
    }
    if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
        throw (Get-LauncherText 'no_executable')
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
        throw (Get-LauncherText 'localapp_missing')
    }
    $locations = @(
        (Join-Path $env:LOCALAPPDATA 'SpeechingWeb')
        (Join-Path $webRoot 'speeching-web-data')
        (Join-Path (Split-Path -Parent $webRoot) 'speeching-web-data')
    )
    $existing = @($locations | Where-Object { Test-Path -LiteralPath (Join-Path $_ 'speeching-web.sqlite3') -PathType Leaf })
    if ($existing.Count -gt 1) {
        throw (Get-LauncherText 'db_multiple')
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
        throw "$(Get-LauncherText 'key_create_error') $failure"
    }
}

function Set-LocalOrigin {
    if ($env:SPEECHING_ORIGIN -or -not $env:SPEECHING_LISTEN) { return }
    try { $uri = [Uri]("http://{0}/" -f $env:SPEECHING_LISTEN) } catch {
        throw (Get-LauncherText 'origin_required')
    }
    if ($uri.Host -notin @('127.0.0.1', '[::1]', '::1') -or $uri.Port -eq 0) {
        throw (Get-LauncherText 'origin_required')
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
        if ([string]::IsNullOrWhiteSpace($env:USERPROFILE)) { throw (Get-LauncherText 'profile_missing') }
        $env:SPEECHING_MASTER_KEY_FILE = Join-Path $env:USERPROFILE '.config\speeching-web\master.key'
    }
    if ($env:SPEECHING_MASTER_KEY_FILE) {
        $env:SPEECHING_MASTER_KEY_FILE = Resolve-WebPath $env:SPEECHING_MASTER_KEY_FILE
        if (-not (Test-Path -LiteralPath $env:SPEECHING_MASTER_KEY_FILE -PathType Leaf)) {
            if ((Test-Path -LiteralPath $database) -or (Test-Path -LiteralPath "$database-wal")) {
                throw (Get-LauncherText 'key_missing')
            }
            New-PrivateMasterKey $env:SPEECHING_MASTER_KEY_FILE
            Write-Host (Get-LauncherText 'key_created')
        }
    }
    Ensure-PrivateDirectory $env:SPEECHING_WEB_DATA
    return $database
}

function Invoke-AccountCommand([string[]]$Arguments) {
    if ($Arguments.Count -ne 3 -or $Arguments[0] -ne 'user' -or $Arguments[1] -ne 'add') {
        throw (Get-LauncherText 'usage_windows')
    }
    & $executable @Arguments
    if ($LASTEXITCODE -ne 0) { throw (Get-LauncherText 'account_save_failed') }
}

function Initialize-Account([string]$Database) {
    $mode = if ($env:SPEECHING_AUTH) { $env:SPEECHING_AUTH } else { 'builtin' }
    if ($mode -ne 'builtin' -or $env:SPEECHING_ALLOW_REGISTRATION -ne 'false' -or (Test-Path -LiteralPath $Database)) { return }
    Write-Host (Get-LauncherText 'first_account_windows')
    $username = Read-Host (Get-LauncherText 'username')
    if ([string]::IsNullOrWhiteSpace($username)) { throw (Get-LauncherText 'account_cancel_windows') }
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
        if ($AuthMode -notin @('builtin', 'off')) { throw (Get-LauncherText 'auth_argument') }
        $env:SPEECHING_AUTH = $AuthMode
        [IO.File]::WriteAllText($setting, $AuthMode)
    } elseif ($env:SPEECHING_AUTH) {
        return
    } elseif (Test-Path -LiteralPath $setting -PathType Leaf) {
        $env:SPEECHING_AUTH = [IO.File]::ReadAllText($setting).Trim()
    } else {
        Write-Host (Get-LauncherText 'auth_intro')
        Write-Host (Get-LauncherText 'auth_login')
        Write-Host (Get-LauncherText 'auth_guest')
        $choice = Normalize-AuthenticationChoice ([string](Read-Host (Get-LauncherText 'auth_choice')))
        switch ($choice) {
            { $_ -eq '' -or $_ -eq '1' } { $env:SPEECHING_AUTH = 'builtin' }
            '2' { $env:SPEECHING_AUTH = 'off' }
            default { throw (Get-LauncherText 'auth_invalid') }
        }
        [IO.File]::WriteAllText($setting, $env:SPEECHING_AUTH)
    }
    if ($env:SPEECHING_AUTH -notin @('builtin', 'off')) {
        throw (Get-LauncherText 'auth_saved_invalid')
    }
}

function Open-PracticeTab([string]$Url) {
    $browser = $env:SPEECHING_WEB_BROWSER
    if ($browser -and -not (Test-Path -LiteralPath $browser -PathType Leaf)) {
        Write-Host (Get-LauncherText 'browser_not_found' @($Url))
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
        Write-Host (Get-LauncherText 'browser_open_failed' @($Url))
    } finally {
        $env:SPEECHING_MASTER_KEY = $masterKey
        $env:SPEECHING_PROXY_SECRET = $proxySecret
    }
}

function Start-WebService {
    $url = if ($env:SPEECHING_ORIGIN) { $env:SPEECHING_ORIGIN.TrimEnd('/') } else { 'http://127.0.0.1:8787' }
    Write-Host (Get-LauncherText 'starting_windows' @($url))
    Write-Host (Get-LauncherText 'keep_open')
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
        if ($LASTEXITCODE -ne 0) { throw (Get-LauncherText 'service_exit' @($LASTEXITCODE)) }
    } finally {
        $ErrorActionPreference = $previousPreference
    }
}

try {
    [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
    Initialize-Language
    Push-Location -LiteralPath $webRoot
    $locationPushed = $true
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
    if ($env:SPEECHING_LAUNCHER_NO_PAUSE -ne '1' -and $script:messages.ContainsKey('press_enter')) {
        [void](Read-Host (Get-LauncherText 'press_enter'))
    }
    exit 1
} finally {
    if ($locationPushed) { Pop-Location }
    [Console]::OutputEncoding = $previousOutputEncoding
}
