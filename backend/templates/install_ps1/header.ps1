param(
  [string]$Client = $env:NEOGATE_CLIENT,
  [switch]$Yes,
  [switch]$Help
)

$ErrorActionPreference = 'Stop'

try {
  Add-Type -AssemblyName System.Net.Http
} catch {
  Write-Host "System.Net.Http is required by the NeoGate installer. $($_.Exception.Message)" -ForegroundColor Red
  throw "System.Net.Http is required by the NeoGate installer. $($_.Exception.Message)"
}

$AppName = 'NeoGate'
$ProviderId = 'neogate'
$ProviderName = 'NeoGate'
$DefaultBaseUrl = '__NEOGATE_DEFAULT_BASE_URL__'
$DefaultCodexModel = 'gpt-5.5'
$DefaultClaudeModel = 'claude-sonnet-4-5'
$BaseUrl = $DefaultBaseUrl
$CodexModel = $DefaultCodexModel
$ClaudeModel = $DefaultClaudeModel
$CodexModelExplicit = $false
$ClaudeModelExplicit = $false
$ApiKey = $env:NEOGATE_API_KEY
$ClientExplicit = -not [string]::IsNullOrEmpty($Client)
$SkipInstall = $false
$SkipRelayTest = $false
$DryRun = $false
if (-not $Yes -and $env:NEOGATE_ASSUME_YES -eq '1') { $Yes = $true }

function Detect-Locale {
  $candidates = @($env:NEOGATE_LOCALE, $env:LC_ALL, $env:LC_MESSAGES, $env:LANGUAGE, $env:LANG, [CultureInfo]::CurrentUICulture.Name, (Get-Culture).Name)
  foreach ($candidate in $candidates) {
    if (-not $candidate) { continue }
    $first = ($candidate -split ':')[0]
    $normalized = $first.ToLower() -replace '_', '-' -replace '\..*', ''
    switch -Wildcard ($normalized) {
      'zh' { $script:InstallLang = 'zh'; return }
      'zh-*' { $script:InstallLang = 'zh'; return }
      'cmn' { $script:InstallLang = 'zh'; return }
      'cmn-*' { $script:InstallLang = 'zh'; return }
      'yue' { $script:InstallLang = 'zh'; return }
      'yue-*' { $script:InstallLang = 'zh'; return }
      'en' { $script:InstallLang = 'en'; return }
      'en-*' { $script:InstallLang = 'en'; return }
    }
  }
  $script:InstallLang = 'en'
}

$InstallLang = 'en'

$MsgEn = @{
  client_required = '--client requires codex or claude'
  unknown_option = 'Unknown option: {0}'
  invalid_client = 'Unsupported client: {0}. Choose codex or claude.'
  cmd_required = '{0} is required'
  no_tty_api_key = 'No interactive TTY found. Set NEOGATE_API_KEY for non-interactive installs.'
  no_tty_client = 'No interactive TTY found. Set NEOGATE_CLIENT=codex or NEOGATE_CLIENT=claude for non-interactive installs.'
  no_tty_model = 'No interactive TTY found, so the installer cannot choose a model. Re-run in an interactive terminal.'
  read_api_key_failed = 'Failed to read API key'
  empty_api_key = 'API key cannot be empty'
  read_client_failed = 'Failed to read client selection'
  read_model_failed = 'Failed to read model selection'
  no_tty_yes = 'No interactive TTY found. Re-run with --yes or NEOGATE_ASSUME_YES=1.'
  elevated_missing = 'This step needs elevated privileges, but sudo is not available.'
  key_verified = 'API key verified'
  key_rejected = 'API key was rejected'
  reenter_api_key = 'Please enter the API key again.'
  key_loaded = 'Reusing API key from previous config'
  model_current_label = ' (current)'
  client_inferred = 'Using previously configured client: {0}'
  verify_not_found = 'Verification endpoint was not found. Make sure NeoGate is up to date and BaseUrl is correct.'
  connect_failed = 'Could not connect to {0}'
  verify_failed = 'API key verification failed with HTTP {0}'
  node_found = 'Node.js {0}'
  npm_found = 'npm {0}'
  node_missing_disabled = 'Node.js/npm is missing and installation is disabled.'
  node_missing_prompt = 'Node.js/npm is missing. Install it now?'
  node_required = 'Node.js/npm is required.'
  homebrew_required = 'Homebrew is required to install Node.js automatically on macOS.'
  unsupported_pkg = 'Unsupported Linux package manager. Install Node.js and npm, then re-run this script.'
  unsupported_os = 'Unsupported operating system: {0}'
  unsupported_arch = 'Unsupported CPU architecture: {0}'
  node_lts_failed = 'Failed to determine Node.js LTS version'
  node_downloading = 'Downloading Node.js {0} from China mirror...'
  node_path_missing = 'Node.js installation did not put node on PATH. Open a new terminal, then re-run this script.'
  npm_path_missing = 'Node.js installation did not put npm on PATH. Open a new terminal, then re-run this script.'
  node_install_retrying = 'Node.js/npm is still not usable; trying to repair the existing Node.js installation.'
  node_install_failed = 'Node.js/npm is still not usable. Repair or reinstall Node.js LTS, open a new terminal, then re-run this script.'
  node_installed = 'Node.js {0}'
  codex_found = 'Codex CLI {0}'
  codex_missing_disabled = 'Codex CLI is missing and installation is disabled.'
  codex_missing_prompt = 'Codex CLI is missing. Install @openai/codex with npm now?'
  codex_required = 'Codex CLI is required.'
  npm_global_failed = 'npm global install failed.'
  codex_sudo_prompt = 'Retry Codex CLI installation with sudo?'
  codex_install_failed = 'Codex CLI installation failed.'
  codex_path_missing = 'Codex CLI was installed but is not on PATH. Open a new terminal, then re-run this script.'
  codex_installed = 'Codex CLI {0}'
  claude_found = 'Claude Code {0}'
  claude_missing_disabled = 'Claude Code is missing and installation is disabled.'
  claude_missing_prompt = 'Claude Code is missing. Install @anthropic-ai/claude-code with npm now?'
  claude_required = 'Claude Code is required.'
  claude_sudo_prompt = 'Retry Claude Code installation with sudo?'
  claude_install_failed = 'Claude Code installation failed.'
  claude_path_missing = 'Claude Code was installed but is not on PATH. Open a new terminal, then re-run this script.'
  claude_installed = 'Claude Code {0}'
  backup_file = 'Backup file: {0}'
  dry_run_preview = 'Dry run: generated Codex config.toml / auth.json preview'
  dry_run_claude_preview = 'Dry run: generated Claude Code config preview'
  config_updated = 'Config updated'
  claude_config_updated = 'Config updated'
  relay_skipped = 'Skipping final relay test'
  relay_testing = 'Requesting {0}/responses'
  chat_relay_testing = 'Requesting {0}/chat/completions'
  claude_relay_testing = 'Requesting {0}/v1/messages'
  relay_succeeded = 'Gateway relay test succeeded'
  responses_relay_succeeded = 'Responses API relay test succeeded'
  chat_relay_succeeded = 'Chat Completions relay test succeeded'
  responses_failed_chat_succeeded = 'Responses API test failed, but Chat Completions succeeded. The current Codex config uses responses mode — check compatibility.'
  both_relay_failed = 'Both Responses API and Chat Completions tests failed.'
  relay_rejected = 'Gateway relay rejected the API key with HTTP {0}'
  relay_failed = 'Test failed: HTTP {0}. The upstream or price config for model {1} may not be ready.'
  relay_upstream_hint = 'The upstream or price config for model {0} may not be ready.'
  relay_internal_hint = 'The gateway returned an internal error. Check backend logs, price config for model ''{0}'', and whether the matching upstream channel/key is enabled and healthy.'
  relay_response_detail = 'Gateway response: {0}'
  fetching_models = 'Fetching {0} models...'
  models_rejected = 'Model list endpoint rejected the API key with HTTP {0}'
  models_not_found = 'Model list endpoint was not found. Make sure NeoGate is up to date and BaseUrl is correct.'
  models_failed = 'Failed to fetch model list with HTTP {0}'
  models_empty = 'No models are available. Ask an admin to configure an available upstream channel and price.'
  installer_title = '{0} installer'
  switch_or_reinstall_prompt = 'Choose [1]'
  switch_option = '1. Switch model'
  change_key_option = '2. Change API key'
  reinstall_option = '3. Reinstall'
  switch_model = 'Switch model'
  change_api_key = 'Change API key'
  keeping_model = 'Keeping current model: {0}'
  model_switched = 'Model switched to {0}'
  api_key_changed = 'API key updated'
  step_verify_key = 'Verify API key'
  step_choose_client = 'Choose client'
  step_choose_model = 'Choose default model'
  step_check_tools = 'Check dependencies'
  step_write_config = 'Write config'
  step_test_gateway = 'Test gateway'
  gateway_url = 'Gateway base URL: {0}'
  config_summary = 'Config summary'
  summary_client_label = 'Client'
  summary_base_url_label = 'Base URL'
  summary_model_label = 'Default model'
  summary_config_file_label = 'Config file'
  codex_model = 'Codex model: {0}'
  claude_model = 'Claude Code model: {0}'
  api_key_prompt = 'Enter API key: '
  choose_client_codex = '1. Codex CLI'
  choose_client_claude = '2. Claude Code'
  choose_client_prompt = 'Choose client [1-2]'
  choose_model_title = 'Choose the default model (press Enter for 1):'
  choose_model_prompt = 'Enter number'
  invalid_model = 'Unsupported model selection: {0}'
  install_tools_prompt = 'Check dependencies?'
  update_config_prompt = 'Write config?'
  install_tools_skipped = 'Skipped dependency check/install'
  config_skipped = 'Skipped config update'
  configured = 'Codex CLI configured'
  claude_configured = 'Claude Code configured'
  try_codex = 'Try: codex --version'
  try_claude = 'Try: claude --version'
  not_generated = 'install script was not dynamically generated. Use the install command from the NeoGate page.'
  node_pkg_missing_win = 'Could not find winget or choco. Install Node.js LTS, then re-run this script.'
  ps1_input_hidden = '(input hidden)'
  installer_title_win = 'Windows installer'
}

$MsgZh = @{
  client_required = '--client 需要 codex 或 claude'
  unknown_option = '未知选项：{0}'
  invalid_client = '不支持的客户端：{0}。请选择 codex 或 claude。'
  cmd_required = '{0} 是必需命令'
  no_tty_api_key = '没有可交互的 TTY。非交互安装请设置 NEOGATE_API_KEY。'
  no_tty_client = '没有可交互的 TTY。非交互安装请设置 NEOGATE_CLIENT=codex 或 NEOGATE_CLIENT=claude。'
  no_tty_model = '没有可交互的 TTY，无法选择模型。请在交互式终端中重新运行。'
  read_api_key_failed = '读取 API 密钥失败'
  empty_api_key = 'API 密钥不能为空'
  read_client_failed = '读取客户端选择失败'
  read_model_failed = '读取模型选择失败'
  no_tty_yes = '没有可交互的 TTY。请用 --yes 或 NEOGATE_ASSUME_YES=1 重新运行。'
  elevated_missing = '此步骤需要管理员权限，但 sudo 不可用。'
  key_verified = 'API 密钥验证成功'
  key_rejected = 'API 密钥被拒绝'
  reenter_api_key = '请重新输入 API 密钥。'
  key_loaded = '已从本地配置读取 API 密钥'
  model_current_label = '（当前）'
  client_inferred = '使用已配置的客户端：{0}'
  verify_not_found = '找不到验证接口。请确认 NeoGate 已更新且 BaseUrl 正确。'
  connect_failed = '无法连接到 {0}'
  verify_failed = 'API 密钥验证失败，HTTP {0}'
  node_found = 'Node.js {0}'
  npm_found = 'npm {0}'
  node_missing_disabled = '缺少 Node.js/npm，且安装已禁用。'
  node_missing_prompt = '缺少 Node.js/npm。现在安装吗？'
  node_required = '需要 Node.js/npm。'
  homebrew_required = 'macOS 自动安装 Node.js 需要 Homebrew。'
  unsupported_pkg = '不支持当前 Linux 包管理器。请安装 Node.js 和 npm 后重新运行此脚本。'
  unsupported_os = '不支持的操作系统：{0}'
  unsupported_arch = '不支持的 CPU 架构：{0}'
  node_lts_failed = '未能获取 Node.js LTS 版本'
  node_downloading = '正在从国内镜像下载 Node.js {0}...'
  node_path_missing = 'Node.js 安装后未在 PATH 中找到 node。请打开新终端后重新运行此脚本。'
  npm_path_missing = 'Node.js 安装后未在 PATH 中找到 npm。请打开新终端后重新运行此脚本。'
  node_install_retrying = 'Node.js/npm 仍不可用，正在尝试修复现有 Node.js 安装。'
  node_install_failed = 'Node.js/npm 仍不可用。请修复或重新安装 Node.js LTS，打开新终端后重新运行此脚本。'
  node_installed = 'Node.js {0}'
  codex_found = 'Codex CLI {0}'
  codex_missing_disabled = '缺少 Codex CLI，且安装已禁用。'
  codex_missing_prompt = '缺少 Codex CLI。现在用 npm 安装 @openai/codex 吗？'
  codex_required = '需要 Codex CLI。'
  npm_global_failed = 'npm 全局安装失败。'
  codex_sudo_prompt = '是否使用 sudo 重试安装 Codex CLI？'
  codex_install_failed = 'Codex CLI 安装失败。'
  codex_path_missing = 'Codex CLI 安装后未在 PATH 中找到 codex。请打开新终端后重新运行此脚本。'
  codex_installed = 'Codex CLI {0}'
  claude_found = 'Claude Code {0}'
  claude_missing_disabled = '缺少 Claude Code，且安装已禁用。'
  claude_missing_prompt = '缺少 Claude Code。现在用 npm 安装 @anthropic-ai/claude-code 吗？'
  claude_required = '需要 Claude Code。'
  claude_sudo_prompt = '是否使用 sudo 重试安装 Claude Code？'
  claude_install_failed = 'Claude Code 安装失败。'
  claude_path_missing = 'Claude Code 安装后未在 PATH 中找到 claude。请打开新终端后重新运行此脚本。'
  claude_installed = 'Claude Code {0}'
  backup_file = '备份文件：{0}'
  dry_run_preview = '试运行：生成的 Codex config.toml / auth.json 预览'
  dry_run_claude_preview = '试运行：生成的 Claude Code 配置预览'
  config_updated = '配置已更新'
  claude_config_updated = '配置已更新'
  relay_skipped = '跳过最终转发测试'
  relay_testing = '正在请求 {0}/responses'
  chat_relay_testing = '正在请求 {0}/chat/completions'
  claude_relay_testing = '正在请求 {0}/v1/messages'
  relay_succeeded = '网关转发测试成功'
  responses_relay_succeeded = 'Responses API 转发测试成功'
  chat_relay_succeeded = 'Chat Completions 转发测试成功'
  responses_failed_chat_succeeded = 'Responses API 测试失败，但 Chat Completions 测试成功。当前 Codex 配置使用 responses 模式，请注意兼容性。'
  both_relay_failed = 'Responses API 和 Chat Completions 测试均未通过。'
  relay_rejected = '网关转发拒绝了 API 密钥，HTTP {0}'
  relay_failed = '测试未通过：HTTP {0}。模型 {1} 的上游或价格配置可能还没就绪。'
  relay_upstream_hint = '模型 {0} 的上游或价格配置可能还没就绪。'
  relay_internal_hint = '网关返回内部错误。请检查后端日志、模型 ''{0}'' 的价格配置，以及对应上游通道和 Key 是否启用且健康。'
  relay_response_detail = '网关返回：{0}'
  fetching_models = '正在获取 {0} 可用模型...'
  models_rejected = '模型列表接口拒绝了 API 密钥，HTTP {0}'
  models_not_found = '找不到模型列表接口。请确认 NeoGate 已更新且 BaseUrl 正确。'
  models_failed = '获取模型列表失败，HTTP {0}'
  models_empty = '当前没有可用模型。请联系管理员配置可用上游通道和价格。'
  installer_title = '{0} 安装器'
  switch_or_reinstall_prompt = '请选择 [1]'
  switch_option = '1. 切换模型'
  change_key_option = '2. 更换 API Key'
  reinstall_option = '3. 重新安装'
  switch_model = '切换模型'
  change_api_key = '更换 API Key'
  keeping_model = '保留当前模型：{0}'
  model_switched = '模型已切换为 {0}'
  api_key_changed = 'API Key 已更新'
  step_verify_key = '验证 API 密钥'
  step_choose_client = '选择客户端'
  step_choose_model = '选择默认模型'
  step_check_tools = '检查依赖'
  step_write_config = '写入配置'
  step_test_gateway = '测试网关'
  gateway_url = '网关基础 URL：{0}'
  config_summary = '配置摘要'
  summary_client_label = '客户端'
  summary_base_url_label = 'Base URL'
  summary_model_label = '默认模型'
  summary_config_file_label = '配置文件'
  codex_model = 'Codex 模型：{0}'
  claude_model = 'Claude Code 模型：{0}'
  api_key_prompt = '请输入 API 密钥：'
  choose_client_codex = '1. Codex CLI'
  choose_client_claude = '2. Claude Code'
  choose_client_prompt = '请选择客户端 [1-2]'
  choose_model_title = '请选择默认模型（回车使用 1）：'
  choose_model_prompt = '请输入编号'
  invalid_model = '不支持的模型选择：{0}'
  install_tools_prompt = '检查依赖？'
  update_config_prompt = '写入配置？'
  install_tools_skipped = '已跳过检查/安装依赖'
  config_skipped = '已跳过配置更新'
  configured = 'Codex CLI 已配置完成'
  claude_configured = 'Claude Code 已配置完成'
  try_codex = '试试：codex --version'
  try_claude = '试试：claude --version'
  not_generated = '安装脚本未经动态生成。请使用 NeoGate 页面上的安装命令。'
  node_pkg_missing_win = '找不到 winget 或 choco。请安装 Node.js LTS 后重新运行此脚本。'
  ps1_input_hidden = '(输入已隐藏)'
  installer_title_win = 'Windows 安装器'
}

function Get-Message {
  param(
    [Parameter(Mandatory=$true, Position=0)][string]$Key,
    [Parameter(ValueFromRemainingArguments=$true)][object[]]$MessageArgs
  )
  $table = if ($InstallLang -eq 'zh') { $MsgZh } else { $MsgEn }
  $fmt = $table[$Key]
  if (-not $fmt) { return $Key }
  if ($MessageArgs -and $MessageArgs.Count -gt 0) {
    return ($fmt -f $MessageArgs)
  }
  return $fmt
}

function Show-Usage {
  if ($InstallLang -eq 'zh') {
    Write-Host @"
NeoGate Windows 安装器
用法：
  irm __NEOGATE_INSTALL_ORIGIN__/install.ps1 | iex

选项：
  -Client CLIENT       codex 或 claude
  -Yes                 API 密钥验证后不再询问确认，直接继续

环境变量：
  NEOGATE_API_KEY, NEOGATE_CLIENT, NEOGATE_ASSUME_YES=1, CODEX_HOME, CLAUDE_HOME
"@
    return
  }

  Write-Host @"
NeoGate Windows installer
Usage:
  irm __NEOGATE_INSTALL_ORIGIN__/install.ps1 | iex

Options:
  -Client CLIENT       codex or claude
  -Yes                 Continue without confirmation prompts after API key verification

Environment variables:
  NEOGATE_API_KEY, NEOGATE_CLIENT, NEOGATE_ASSUME_YES=1, CODEX_HOME, CLAUDE_HOME
"@
}

Detect-Locale

if ($Help) {
  Show-Usage
  return
}

function Step([string]$Text) {
  $script:InstallStep += 1
  Write-Host ''
  Write-Host "[$script:InstallStep/$script:InstallTotalSteps] $Text" -ForegroundColor Blue
}

function Detail([string]$Text) {
  Write-Host "  $Text"
}

function Success([string]$Text) {
  Write-Host "OK $Text" -ForegroundColor Green
}

function Warn([string]$Text) {
  Write-Warning $Text
}

function Fail([string]$Text) {
  throw $Text
}

function Confirm-DefaultYes([string]$Prompt) {
  if ($Yes) { return $true }
  $answer = Read-Host "$Prompt [Y/n]"
  return $answer -eq '' -or $answer -match '^(y|yes)$'
}
