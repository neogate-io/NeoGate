$NodeMirror = 'https://registry.npmmirror.com/-/binary/node'
$NpmRegistry = 'https://registry.npmmirror.com'

function Read-JsonField([string]$Path, [string]$Field) {
  if (-not (Test-Path $Path)) { return $null }
  try {
    $obj = Get-Content $Path -Raw | ConvertFrom-Json
    if ($obj.$Field) { return [string]$obj.$Field }
  } catch {
    return $null
  }
  return $null
}

function Read-JsonEnvField([string]$Path, [string]$Field) {
  if (-not (Test-Path $Path)) { return $null }
  try {
    $obj = Get-Content $Path -Raw | ConvertFrom-Json
    if ($obj.env -and $obj.env.$Field) { return [string]$obj.env.$Field }
  } catch {
    return $null
  }
  return $null
}

# Reads existing Codex ~/.codex/auth.json + config.toml and Claude ~/.claude/settings.json
# and remembers previously-used API keys/models.
function Load-ExistingCredentials {
  $script:LoadedCodexKey = $null
  $script:LoadedClaudeKey = $null
  $script:LoadedCodexModel = $null
  $script:LoadedClaudeModel = $null
  $script:HasExistingConfig = $false

  $codexHome = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path $env:USERPROFILE '.codex' }
  $claudeHome = if ($env:CLAUDE_HOME) { $env:CLAUDE_HOME } else { Join-Path $env:USERPROFILE '.claude' }
  $codexAuth = Join-Path $codexHome 'auth.json'
  $codexConfig = Join-Path $codexHome 'config.toml'
  $claudeConfig = Join-Path $claudeHome 'settings.json'

  if (Test-Path $codexAuth) {
    $script:LoadedCodexKey = Read-JsonField $codexAuth 'OPENAI_API_KEY'
  }
  if (Test-Path $claudeConfig) {
    $script:LoadedClaudeKey = Read-JsonEnvField $claudeConfig 'ANTHROPIC_AUTH_TOKEN'
  }

  if (Test-Path $codexConfig) {
    $modelLine = Get-Content $codexConfig | Where-Object { $_ -match '^\s*model\s*=\s*"([^"]*)"' } | Select-Object -First 1
    if ($modelLine -and $modelLine -match '"([^"]*)"') { $script:LoadedCodexModel = $Matches[1] }
  }

  if (Test-Path $claudeConfig) {
    $script:LoadedClaudeModel = Read-JsonField $claudeConfig 'model'
  }

  $codexPresent = [bool]($LoadedCodexKey) -or [bool]($LoadedCodexModel)
  $claudePresent = [bool]($LoadedClaudeKey) -or [bool]($LoadedClaudeModel)
  if ($codexPresent -or $claudePresent) { $script:HasExistingConfig = $true }
}

function Get-LoadedApiKeyForSelectedClient {
  if ($Client -eq 'claude') { return $LoadedClaudeKey }
  return $LoadedCodexKey
}

function Use-ApiKeyForSelectedClient {
  if ($ApiKey) { return }

  $loadedKey = Get-LoadedApiKeyForSelectedClient
  if ($loadedKey) {
    $script:ApiKey = $loadedKey
    Detail (Get-Message key_loaded)
  }
}

function Normalize-BaseUrl {
  $script:BaseUrl = $BaseUrl.Trim().TrimEnd('/')
  if ($script:BaseUrl.EndsWith('/anthropic')) {
    $script:BaseUrl = $script:BaseUrl.Substring(0, $script:BaseUrl.Length - '/anthropic'.Length)
  }
  if (-not $script:BaseUrl.EndsWith('/v1')) {
    $script:BaseUrl = "$script:BaseUrl/v1"
  }
  $script:ApiRoot = $script:BaseUrl.Substring(0, $script:BaseUrl.Length - '/v1'.Length)
  $script:AnthropicBaseUrl = "$script:ApiRoot/anthropic"
}

function Read-SecretText([string]$Prompt) {
  $secure = Read-Host "$Prompt $(Get-Message ps1_input_hidden)" -AsSecureString
  $ptr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)
  try {
    return [Runtime.InteropServices.Marshal]::PtrToStringBSTR($ptr)
  } finally {
    if ($ptr -ne [IntPtr]::Zero) {
      [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($ptr)
    }
  }
}

function Invoke-NeoGateRequest {
  param(
    [Parameter(Mandatory=$true)][string]$Uri,
    [string]$Method = 'GET',
    [hashtable]$Headers = @{},
    [string]$Body = $null,
    [string]$ContentType = 'application/json'
  )

  $client = $null
  $request = $null
  $content = $null
  try {
    $client = [System.Net.Http.HttpClient]::new()
    $request = [System.Net.Http.HttpRequestMessage]::new(
      [System.Net.Http.HttpMethod]::new($Method.ToUpperInvariant()),
      [Uri]$Uri
    )

    foreach ($header in $Headers.GetEnumerator()) {
      [void]$request.Headers.TryAddWithoutValidation([string]$header.Key, [string]$header.Value)
    }

    if (-not [string]::IsNullOrEmpty($Body)) {
      $content = [System.Net.Http.StringContent]::new($Body, [System.Text.Encoding]::UTF8, $ContentType)
      $request.Content = $content
    }

    $response = $client.SendAsync($request).GetAwaiter().GetResult()
    $bodyText = $response.Content.ReadAsStringAsync().GetAwaiter().GetResult()
    return @{ Status = [int]$response.StatusCode; Body = $bodyText }
  } catch {
    return @{ Status = 0; Body = $_.Exception.Message }
  } finally {
    if ($null -ne $content) { $content.Dispose() }
    if ($null -ne $request) { $request.Dispose() }
    if ($null -ne $client) { $client.Dispose() }
  }
}

function Find-ApplicationCommand([string]$Name) {
  # Prefer .exe/.cmd shims over PowerShell wrapper scripts. Node's npm.ps1 can
  # be blocked by a restrictive execution policy even when npm.cmd is usable.
  return Get-Command $Name -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
}

function Assert-Command([string]$Name) {
  return $null -ne (Find-ApplicationCommand $Name)
}

function Get-CommandVersion([string]$Name, [string[]]$Arguments = @('--version')) {
  $cmd = Find-ApplicationCommand $Name
  if (-not $cmd) { return $null }

  try {
    $output = & $cmd.Path @Arguments 2>$null
    if ($LASTEXITCODE -ne 0) { return $null }
    return (@($output) | Where-Object { $_ } | Select-Object -First 1)
  } catch {
    return $null
  }
}

function Get-NpmGlobalPaths {
  $paths = @()
  try {
    $npm = Find-ApplicationCommand 'npm'
    if (-not $npm) { return @() }
    $prefixOutput = & $npm.Path config get prefix 2>$null
    if ($LASTEXITCODE -eq 0) {
      foreach ($prefix in @($prefixOutput)) {
        if ($prefix -and $prefix -ne 'undefined') {
          $prefixPath = [string]$prefix
          $paths += $prefixPath
          $paths += (Join-Path $prefixPath 'bin')
        }
      }
    }
  } catch {
    return @()
  }
  return $paths
}

function Get-ResponseErrorMessage([string]$Body) {
  # Extract the human-readable error message from a JSON response body.
  # Supports both flat {"error": "..."} and nested {"error": {"message": "..."}}.
  if (-not $Body) { return '' }
  try {
    $json = $Body | ConvertFrom-Json -ErrorAction Stop
  } catch {
    return ''
  }
  if ($null -ne $json.error) {
    if ($json.error -is [string]) { return $json.error }
    if ($json.error.message) { return $json.error.message }
  }
  if ($json.message) { return $json.message }
  return ''
}

function Update-SessionPath {
  $paths = @(
    [Environment]::GetEnvironmentVariable('Path', 'Machine'),
    [Environment]::GetEnvironmentVariable('Path', 'User'),
    $env:Path
    Get-NpmGlobalPaths
  ) | Where-Object { $_ }

  $env:Path = (($paths -join ';') -split ';' | Where-Object { $_ } | Select-Object -Unique) -join ';'
}

function Run-Command {
  param([string]$FilePath, [string[]]$Arguments)
  $exitCode = Invoke-CommandStatus $FilePath $Arguments
  if ($exitCode -ne 0) {
    Fail "$FilePath failed with exit code $exitCode"
  }
}

function Invoke-CommandStatus {
  param([string]$FilePath, [string[]]$Arguments)
  if ($DryRun) {
    Write-Host "+ $FilePath $($Arguments -join ' ')"
    return 0
  }
  & $FilePath @Arguments
  return $LASTEXITCODE
}

function Verify-ApiKey {
  $result = Invoke-NeoGateRequest -Uri "$ApiRoot/api/user-key/verify" -Headers @{ authorization = "Bearer $ApiKey" }
  switch ($result.Status) {
    200 { Success (Get-Message key_verified); return $true }
    401 { Warn (Get-Message key_rejected); return $false }
    403 { Warn (Get-Message key_rejected); return $false }
    404 { Fail (Get-Message verify_not_found) }
    0 { Fail "$(Get-Message connect_failed $ApiRoot). $($result.Body)" }
    default { Fail "$(Get-Message verify_failed $result.Status). $($result.Body)" }
  }
}

function Read-AndVerifyApiKey {
  param([switch]$ForcePrompt)
  while ($true) {
    if ($ForcePrompt -or -not $ApiKey) {
      $script:ApiKey = Read-SecretText (Get-Message api_key_prompt)
      $ForcePrompt = $false
    }
    if (-not $ApiKey) {
      Warn (Get-Message empty_api_key)
      $script:ApiKey = $null
      continue
    }
    if (Verify-ApiKey) {
      return
    }
    Warn (Get-Message reenter_api_key)
    $script:ApiKey = $null
  }
}

function Get-LoadedModelForSelectedClient {
  if ($Client -eq 'claude') { return $LoadedClaudeModel }
  return $LoadedCodexModel
}

function Use-LoadedModelForSelectedClient {
  $loadedModel = Get-LoadedModelForSelectedClient
  if (-not $loadedModel) { return $false }
  if ($Client -eq 'claude') {
    $script:ClaudeModel = $loadedModel
  } else {
    $script:CodexModel = $loadedModel
  }
  Detail (Get-Message keeping_model $loadedModel)
  return $true
}

function Normalize-Client([string]$Value) {
  switch -Regex ($Value.ToLowerInvariant()) {
    '^(1|codex)$' { return 'codex' }
    '^(2|claude|claude-code)$' { return 'claude' }
    default { Fail (Get-Message invalid_client $Value) }
  }
}

function Select-Client {
  if ($Client) {
    $script:Client = Normalize-Client $Client
    return
  }

  Write-Host (Get-Message choose_client_codex)
  Write-Host (Get-Message choose_client_claude)
  $answer = Read-Host (Get-Message choose_client_prompt)
  $script:Client = Normalize-Client $answer
}

function Selected-ClientName {
  if ($Client -eq 'claude') { return 'Claude Code' }
  return 'Codex CLI'
}

function Selected-BaseUrl {
  if ($Client -eq 'claude') { return $AnthropicBaseUrl }
  return $BaseUrl
}

function Selected-Model {
  if ($Client -eq 'claude') { return $ClaudeModel }
  return $CodexModel
}

function Selected-ConfigFile {
  if ($Client -eq 'claude') {
    $claudeHome = if ($env:CLAUDE_HOME) { $env:CLAUDE_HOME } else { Join-Path $env:USERPROFILE '.claude' }
    return (Join-Path $claudeHome 'settings.json')
  }
  $codexHome = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path $env:USERPROFILE '.codex' }
  return "$(Join-Path $codexHome 'config.toml'), $(Join-Path $codexHome 'auth.json')"
}

function Get-Models {
  if ($Client -eq 'claude') {
    $result = Invoke-NeoGateRequest -Uri "$AnthropicBaseUrl/v1/messages/models" -Headers @{
      'x-api-key' = $ApiKey
      'anthropic-version' = '2023-06-01'
    }
  } else {
    $result = Invoke-NeoGateRequest -Uri "$BaseUrl/models" -Headers @{ authorization = "Bearer $ApiKey" }
  }

  switch ($result.Status) {
    200 {
      $json = $result.Body | ConvertFrom-Json
      $models = @($json.data | ForEach-Object { $_.id } | Where-Object { $_ })
      if ($models.Count -eq 0) { Fail (Get-Message models_empty) }
      return $models
    }
    401 { Fail (Get-Message models_rejected $result.Status) }
    403 { Fail (Get-Message models_rejected $result.Status) }
    404 { Fail (Get-Message models_not_found) }
    0 { Fail (Get-Message connect_failed (Selected-BaseUrl)) }
    default { Fail "$(Get-Message models_failed $result.Status). $($result.Body)" }
  }
}

function Select-Model {
  $models = @(Get-Models)
  $current = Selected-Model
  $explicit = if ($Client -eq 'claude') { $ClaudeModelExplicit } else { $CodexModelExplicit }

  if ($explicit) {
    if ($models -notcontains $current) { Fail (Get-Message invalid_model $current) }
    return
  }

  $loadedModel = if ($Client -eq 'claude') { $LoadedClaudeModel } else { $LoadedCodexModel }
  $defaultIndex = 1

  Write-Host (Get-Message choose_model_title)
  for ($i = 0; $i -lt $models.Count; $i++) {
    $label = "$($i + 1). $($models[$i])"
    if ($loadedModel -and $models[$i] -eq $loadedModel) {
      $defaultIndex = $i + 1
      $label += (Get-Message model_current_label)
    }
    Write-Host $label
  }
  $answer = Read-Host "$(Get-Message choose_model_prompt) [$defaultIndex]"
  if (-not $answer) { $answer = "$defaultIndex" }
  $index = 0
  if (-not [int]::TryParse($answer, [ref]$index) -or $index -lt 1 -or $index -gt $models.Count) {
    Fail (Get-Message invalid_model $answer)
  }
  if ($Client -eq 'claude') {
    $script:ClaudeModel = $models[$index - 1]
  } else {
    $script:CodexModel = $models[$index - 1]
  }
  Success (Selected-Model)
}

function Get-NodeToolVersions {
  return @{
    Node = Get-CommandVersion 'node'
    Npm = Get-CommandVersion 'npm'
  }
}

function Confirm-NodeReady {
  $versions = Get-NodeToolVersions
  if ($versions.Node -and $versions.Npm) {
    Detail (Get-Message node_found $versions.Node)
    Detail (Get-Message npm_found $versions.Npm)
    return $true
  }
  return $false
}

function Get-NodeLtsVersion {
  $indexUrl = "$NodeMirror/index.json"
  try {
    $client = [System.Net.Http.HttpClient]::new()
    $response = $client.GetStringAsync($indexUrl).GetAwaiter().GetResult()
    $client.Dispose()
    $data = $response | ConvertFrom-Json
    $lts = $data | Where-Object { $_.lts } | Select-Object -First 1
    if (-not $lts) { Fail (Get-Message node_lts_failed) }
    return $lts.version
  } catch {
    Fail (Get-Message connect_failed $indexUrl)
  }
}

function Install-NodeZip {
  $version = Get-NodeLtsVersion
  $zipUrl = "$NodeMirror/$version/node-$version-win-x64.zip"
  $targetDir = if ($env:NEOGATE_NODE_HOME) { $env:NEOGATE_NODE_HOME } else { Join-Path $env:USERPROFILE '.neogate-node' }
  $zipFile = Join-Path $env:TEMP "neogate-node-$version.zip"

  Write-Host (Get-Message node_downloading $version)
  Invoke-WebRequest -Uri $zipUrl -OutFile $zipFile -UseBasicParsing

  if (Test-Path $targetDir) { Remove-Item -Recurse -Force $targetDir }
  New-Item -ItemType Directory -Force -Path $targetDir | Out-Null
  Expand-Archive -Path $zipFile -DestinationPath $targetDir -Force

  # The zip extracts into a top-level node-v*-win-x64/ directory; resolve the
  # real directory that contains node.exe and npm.cmd.
  $nodeExe = Get-ChildItem -Path $targetDir -Recurse -Filter 'node.exe' | Select-Object -First 1
  if (-not $nodeExe) { Fail (Get-Message node_path_missing) }
  $nodeBin = $nodeExe.DirectoryName
  $npmCmd = Join-Path $nodeBin 'npm.cmd'
  if (-not (Test-Path -LiteralPath $npmCmd -PathType Leaf)) { Fail (Get-Message npm_path_missing) }

  $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
  if ($userPath -notlike "*$nodeBin*") {
    $newPath = if ($userPath) { "$nodeBin;$userPath" } else { $nodeBin }
    [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
  }
  # The registry update above is not reflected in this PowerShell process.
  # Keep the extracted directory first so node.exe and npm.cmd are usable now.
  $env:Path = (@(
    $nodeBin
    $env:Path
    [Environment]::GetEnvironmentVariable('Path', 'User')
    [Environment]::GetEnvironmentVariable('Path', 'Machine')
  ) | Where-Object { $_ } | ForEach-Object { $_ -split ';' } |
    Where-Object { $_ } | Select-Object -Unique) -join ';'
  $env:NPM_CONFIG_REGISTRY = $NpmRegistry
  return $nodeBin
}

function Install-Node {
  Update-SessionPath
  if (Confirm-NodeReady) { return }

  if ($SkipInstall) { Fail (Get-Message node_missing_disabled) }
  if (-not (Confirm-DefaultYes (Get-Message node_missing_prompt))) { Fail (Get-Message node_required) }

  $nodeBin = Install-NodeZip
  Update-SessionPath

  # Test the newly extracted executables explicitly. Get-Command can retain a
  # stale command lookup after PATH changes in the same PowerShell session.
  $nodeExe = Join-Path $nodeBin 'node.exe'
  $npmCmd = Join-Path $nodeBin 'npm.cmd'
  $nodeVersion = Get-CommandVersion $nodeExe
  $npmVersion = Get-CommandVersion $npmCmd
  if ($nodeVersion -and $npmVersion) {
    Detail (Get-Message node_found $nodeVersion)
    Detail (Get-Message npm_found $npmVersion)
    return
  }

  if (Confirm-NodeReady) { return }
  $versions = Get-NodeToolVersions
  if (-not $versions.Node) { Fail (Get-Message node_path_missing) }
  if (-not $versions.Npm) { Fail (Get-Message npm_path_missing) }
  Fail (Get-Message node_install_failed)
}

function Install-CodexCli {
  Update-SessionPath
  if (Assert-Command 'codex') {
    Detail (Get-Message codex_found $(& codex --version 2>$null))
    return
  }
  if ($SkipInstall) { Fail (Get-Message codex_missing_disabled) }
  if (-not (Confirm-DefaultYes (Get-Message codex_missing_prompt))) { Fail (Get-Message codex_required) }
  Run-Command 'npm' @('install', '-g', '--registry', $NpmRegistry, '@openai/codex')
  Update-SessionPath
  if (-not (Assert-Command 'codex')) { Fail (Get-Message codex_path_missing) }
  Detail (Get-Message codex_found $(& codex --version 2>$null))
}

function Install-ClaudeCode {
  Update-SessionPath
  if (Assert-Command 'claude') {
    Detail (Get-Message claude_found $(& claude --version 2>$null))
    return
  }
  if ($SkipInstall) { Fail (Get-Message claude_missing_disabled) }
  if (-not (Confirm-DefaultYes (Get-Message claude_missing_prompt))) { Fail (Get-Message claude_required) }
  Run-Command 'npm' @('install', '-g', '--registry', $NpmRegistry, '@anthropic-ai/claude-code')
  Update-SessionPath
  if (-not (Assert-Command 'claude')) { Fail (Get-Message claude_path_missing) }
  Detail (Get-Message claude_found $(& claude --version 2>$null))
}

function Write-JsonFile {
  param([string]$Path, [object]$Value)
  $json = $Value | ConvertTo-Json -Depth 20
  if ($DryRun) {
    Write-Host "# $Path"
    Write-Host $json
    return
  }
  $dir = Split-Path -Parent $Path
  New-Item -ItemType Directory -Force -Path $dir | Out-Null
  if (Test-Path $Path) {
    Copy-Item $Path "$Path.bak-$(Get-Date -Format yyyyMMddHHmmss)"
  }
  [System.IO.File]::WriteAllText($Path, "$json`n", [System.Text.UTF8Encoding]::new($false))
}

function Escape-Toml([string]$Value) {
  return $Value.Replace('\', '\\').Replace('"', '\"')
}

function Escape-TomlKey([string]$Value) {
  return (Escape-Toml $Value)
}

function Write-CodexConfig {
  $codexHome = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path $env:USERPROFILE '.codex' }
  $configFile = Join-Path $codexHome 'config.toml'
  $authFile = Join-Path $codexHome 'auth.json'
  $timestamp = Get-Date -Format yyyyMMddHHmmss
  $providerIdEscaped = Escape-TomlKey $ProviderId

  $existing = ''
  if (Test-Path $configFile) {
    $existing = Get-Content $configFile -Raw
  }
  $lines = @()
  $skip = $false
  foreach ($line in ($existing -split "`r?`n")) {
    if ($line -match '^\s*\[\s*model_providers\s*\.\s*"?neogate"?\s*\]?\s*$') {
      $skip = $true
      continue
    }
    if ($line -match '^\[') { $skip = $false }
    if ($skip) { continue }
    if ($line -match '^(model|model_provider|openai_base_url|preferred_auth_method|model_reasoning_effort|OPENAI_API_KEY)\s*=') { continue }
    $lines += $line
  }

  $next = @(
    "model = `"$(Escape-Toml $CodexModel)`"",
    "model_provider = `"$providerIdEscaped`"",
    "openai_base_url = `"$(Escape-Toml $BaseUrl)`"",
    ''
  ) + $lines + @(
    '',
    "[model_providers.`"$providerIdEscaped`"]",
    "name = `"$(Escape-Toml $ProviderName)`"",
    "base_url = `"$(Escape-Toml $BaseUrl)`"",
    'wire_api = "responses"',
    'requires_openai_auth = false'
  )

  if ($DryRun) {
    Write-Host "# $configFile"
    Write-Host ($next -join [Environment]::NewLine)
  } else {
    New-Item -ItemType Directory -Force -Path $codexHome | Out-Null
    if (Test-Path $configFile) { Copy-Item $configFile "$configFile.bak-$timestamp" }
    [System.IO.File]::WriteAllText(
      $configFile,
      (($next -join [Environment]::NewLine) + [Environment]::NewLine),
      [System.Text.UTF8Encoding]::new($false)
    )
  }

  $auth = @{}
  if (Test-Path $authFile) {
    try {
      $authObject = Get-Content $authFile -Raw | ConvertFrom-Json
      foreach ($property in $authObject.PSObject.Properties) {
        $auth[$property.Name] = $property.Value
      }
    } catch {
      $auth = @{}
    }
  }
  $auth.OPENAI_API_KEY = $ApiKey
  $auth.auth_mode = 'apikey'
  Write-JsonFile -Path $authFile -Value $auth
  Success (Get-Message config_updated)
}

function Write-ClaudeConfig {
  $claudeHome = if ($env:CLAUDE_HOME) { $env:CLAUDE_HOME } else { Join-Path $env:USERPROFILE '.claude' }
  $configFile = Join-Path $claudeHome 'settings.json'
  $settings = [ordered]@{}

  if (Test-Path $configFile) {
    try {
      $settingsObject = Get-Content $configFile -Raw | ConvertFrom-Json
      foreach ($property in $settingsObject.PSObject.Properties) {
        $settings[$property.Name] = $property.Value
      }
    } catch {
      $settings = [ordered]@{}
    }
  }

  $envSettings = [ordered]@{}
  if ($settings.env) {
    foreach ($property in $settings.env.PSObject.Properties) {
      if ($property.Name -ne 'ANTHROPIC_API_KEY') {
        $envSettings[$property.Name] = $property.Value
      }
    }
  }

  $envSettings.ANTHROPIC_BASE_URL = $AnthropicBaseUrl
  $envSettings.ANTHROPIC_AUTH_TOKEN = $ApiKey
  $envSettings.ANTHROPIC_MODEL = $ClaudeModel
  $envSettings.ANTHROPIC_DEFAULT_OPUS_MODEL = $ClaudeModel
  $envSettings.ANTHROPIC_DEFAULT_SONNET_MODEL = $ClaudeModel
  $envSettings.ANTHROPIC_DEFAULT_HAIKU_MODEL = $ClaudeModel
  $envSettings.ANTHROPIC_REASONING_MODEL = $ClaudeModel
  $envSettings.ANTHROPIC_CUSTOM_MODEL_OPTION = $ClaudeModel
  $settings.env = $envSettings
  $settings.model = $ClaudeModel

  Write-JsonFile -Path $configFile -Value $settings
  Success (Get-Message claude_config_updated)
}

function Test-CodexRelay {
  if ($SkipRelayTest) {
    Warn (Get-Message relay_skipped)
    return
  }
  $responsesPayload = @{ model = $CodexModel; input = 'Reply with OK only.'; max_output_tokens = 16 } | ConvertTo-Json -Compress
  $result = Invoke-NeoGateRequest -Uri "$BaseUrl/responses" -Method POST -Headers @{ authorization = "Bearer $ApiKey" } -Body $responsesPayload
  if ($result.Status -ge 200 -and $result.Status -lt 300) {
    Success (Get-Message responses_relay_succeeded)
    return
  } elseif ($result.Status -eq 401 -or $result.Status -eq 403) {
    Fail (Get-Message relay_rejected $result.Status)
  } elseif ($result.Status -eq 0) {
    Fail (Get-Message connect_failed $BaseUrl)
  } else {
    Warn (Get-Message relay_failed $result.Status $CodexModel)
    if ($result.Body) { Detail $result.Body }
  }

  $chatPayload = @{
    model = $CodexModel
    messages = @(@{ role = 'user'; content = 'Reply with OK only.' })
    max_tokens = 16
  } | ConvertTo-Json -Depth 10 -Compress
  $chatResult = Invoke-NeoGateRequest -Uri "$BaseUrl/chat/completions" -Method POST -Headers @{ authorization = "Bearer $ApiKey" } -Body $chatPayload
  if ($chatResult.Status -ge 200 -and $chatResult.Status -lt 300) {
    Success (Get-Message chat_relay_succeeded)
    Warn (Get-Message responses_failed_chat_succeeded)
  } elseif ($chatResult.Status -eq 401 -or $chatResult.Status -eq 403) {
    Fail (Get-Message relay_rejected $chatResult.Status)
  } elseif ($chatResult.Status -eq 0) {
    Fail (Get-Message connect_failed $BaseUrl)
  } else {
    Warn (Get-Message relay_failed $chatResult.Status $CodexModel)
    if ($chatResult.Body) { Detail $chatResult.Body }
    Warn (Get-Message both_relay_failed)
  }
}

function Test-ClaudeRelay {
  if ($SkipRelayTest) {
    Warn (Get-Message relay_skipped)
    return
  }
  $payload = @{
    model = $ClaudeModel
    messages = @(@{ role = 'user'; content = 'Reply with OK only.' })
    max_tokens = 16
  } | ConvertTo-Json -Depth 10 -Compress
  $result = Invoke-NeoGateRequest -Uri "$AnthropicBaseUrl/v1/messages" -Method POST -Headers @{
    authorization = "Bearer $ApiKey"
    'anthropic-version' = '2023-06-01'
  } -Body $payload
  if ($result.Status -ge 200 -and $result.Status -lt 300) {
    Success (Get-Message relay_succeeded)
  } elseif ($result.Status -eq 401 -or $result.Status -eq 403) {
    Fail (Get-Message relay_rejected $result.Status)
  } elseif ($result.Status -eq 0) {
    Fail (Get-Message connect_failed $AnthropicBaseUrl)
  } else {
    Warn (Get-Message relay_failed $result.Status $ClaudeModel)
    if ($result.Body) { Detail $result.Body }
  }
}

function Choose-SwitchModel {
  Write-Host (Get-Message switch_option)
  Write-Host (Get-Message change_key_option)
  Write-Host (Get-Message reinstall_option)
  $answer = Read-Host (Get-Message switch_or_reinstall_prompt)
  # Default (empty) and "1" => switch model; "2" => change API key; "3" => reinstall.
  switch ($answer) {
    '' { return 'switch_model' }
    '1' { return 'switch_model' }
    '2' { return 'change_key' }
    '3' { return 'reinstall' }
    default { return 'switch_model' }
  }
}

function Invoke-SwitchModelFlow {
  Step (Get-Message switch_model)
  Select-Model

  Step (Get-Message step_write_config)
  if ($Client -eq 'claude') {
    Write-ClaudeConfig
    Step (Get-Message step_test_gateway)
    Test-ClaudeRelay
  } else {
    Write-CodexConfig
    Step (Get-Message step_test_gateway)
    Test-CodexRelay
  }

  Success (Get-Message model_switched (Selected-Model))
}

function Invoke-ChangeKeyFlow {
  Step (Get-Message change_api_key)
  $script:ApiKey = $null
  Read-AndVerifyApiKey -ForcePrompt

  if (-not (Use-LoadedModelForSelectedClient)) {
    Step (Get-Message step_choose_model)
    Select-Model
  }

  Step (Get-Message step_write_config)
  if ($Client -eq 'claude') {
    Write-ClaudeConfig
    Step (Get-Message step_test_gateway)
    Test-ClaudeRelay
  } else {
    Write-CodexConfig
    Step (Get-Message step_test_gateway)
    Test-CodexRelay
  }

  Success (Get-Message api_key_changed)
}

function Invoke-FullFlow {
  Step (Get-Message step_choose_model)
  Select-Model

  Write-Host ''
  Write-Host (Get-Message config_summary)
  Write-Host "$(Get-Message summary_client_label)     $(Selected-ClientName)"
  Write-Host "$(Get-Message summary_base_url_label)   $(Selected-BaseUrl)"
  Write-Host "$(Get-Message summary_model_label)      $(Selected-Model)"
  Write-Host "$(Get-Message summary_config_file_label)     $(Selected-ConfigFile)"

  Step (Get-Message step_check_tools)
  if (Confirm-DefaultYes (Get-Message install_tools_prompt)) {
    Install-Node
    if ($Client -eq 'claude') {
      Install-ClaudeCode
    } else {
      Install-CodexCli
    }
  } else {
    Warn (Get-Message install_tools_skipped)
  }

  Step (Get-Message step_write_config)
  if (Confirm-DefaultYes (Get-Message update_config_prompt)) {
    if ($Client -eq 'claude') {
      Write-ClaudeConfig
      Step (Get-Message step_test_gateway)
      Test-ClaudeRelay
      Success (Get-Message claude_configured)
    } else {
      Write-CodexConfig
      Step (Get-Message step_test_gateway)
      Test-CodexRelay
      Success (Get-Message configured)
    }
  } else {
    Warn (Get-Message config_skipped)
  }
}
