#!/usr/bin/env bash
set -Eeuo pipefail

APP_NAME="NeoGate"
PROVIDER_ID="neogate"
PROVIDER_NAME="NeoGate"
DEFAULT_BASE_URL="__NEOGATE_DEFAULT_BASE_URL__"
DEFAULT_CODEX_MODEL="gpt-5.5"
DEFAULT_CLAUDE_MODEL="claude-sonnet-4-5"

BASE_URL="$DEFAULT_BASE_URL"
CODEX_MODEL="$DEFAULT_CODEX_MODEL"
CLAUDE_MODEL="$DEFAULT_CLAUDE_MODEL"
CODEX_MODEL_EXPLICIT=0
CLAUDE_MODEL_EXPLICIT=0
API_KEY="${NEOGATE_API_KEY:-}"
CLIENT="${NEOGATE_CLIENT:-}"
CLIENT_EXPLICIT=0
[[ -n "$CLIENT" ]] && CLIENT_EXPLICIT=1
ASSUME_YES="${NEOGATE_ASSUME_YES:-0}"
SKIP_INSTALL=0
SKIP_RELAY_TEST=0
DRY_RUN=0
TMP_DIR=""
INSTALL_LOCALE="${NEOGATE_LOCALE:-}"
INSTALL_LANG="en"
INSTALL_STEP=0
INSTALL_TOTAL_STEPS=6
NODE_REQUIRED_MAJOR=22

if [[ -t 1 ]]; then
  COLOR_BLUE=$'\033[34m'
  COLOR_GREEN=$'\033[32m'
  COLOR_YELLOW=$'\033[33m'
  COLOR_RED=$'\033[31m'
  COLOR_RESET=$'\033[0m'
else
  COLOR_BLUE=""
  COLOR_GREEN=""
  COLOR_YELLOW=""
  COLOR_RED=""
  COLOR_RESET=""
fi

log() {
  printf '%s\n' "$*"
}

step() {
  INSTALL_STEP=$((INSTALL_STEP + 1))
  printf '\n'
  printf '%s[%s/%s]%s %s\n' "$COLOR_BLUE" "$INSTALL_STEP" "$INSTALL_TOTAL_STEPS" "$COLOR_RESET" "$*"
}

space() {
  printf '\n'
}

detail() {
  printf '  %s\n' "$*"
}

detail_ok() {
  printf '%s✓%s %s\n' "$COLOR_GREEN" "$COLOR_RESET" "$*"
}

success() {
  printf '%s✓%s %s\n' "$COLOR_GREEN" "$COLOR_RESET" "$*"
}

warn() {
  printf '%s!%s %s\n' "$COLOR_YELLOW" "$COLOR_RESET" "$*" >&2
}

die() {
  printf '%s!%s %s\n' "$COLOR_RED" "$COLOR_RESET" "$*" >&2
  exit 1
}

detect_locale() {
  local candidate normalized first

  for candidate in "$INSTALL_LOCALE" "${LC_ALL:-}" "${LC_MESSAGES:-}" "${LANGUAGE:-}" "${LANG:-}"; do
    [[ -n "$candidate" ]] || continue
    first="${candidate%%:*}"
    normalized="$(printf '%s' "$first" | tr '[:upper:]' '[:lower:]')"
    normalized="${normalized//_/-}"
    normalized="${normalized%%.*}"

    case "$normalized" in
      zh|zh-*|cmn|cmn-*|yue|yue-*)
        INSTALL_LANG="zh"
        return 0
        ;;
      en|en-*)
        INSTALL_LANG="en"
        return 0
        ;;
    esac
  done

  INSTALL_LANG="en"
}

message() {
  local key="$1"
  shift || true

  if [[ "$INSTALL_LANG" == "zh" ]]; then
    case "$key" in
      client_required) printf '%s' "--client 需要 codex 或 claude" ;;
      unknown_option) printf '未知选项：%s' "$1" ;;
      invalid_client) printf '不支持的客户端：%s。请选择 codex 或 claude。' "$1" ;;
      cmd_required) printf '%s 是必需命令' "$1" ;;
      no_tty_api_key) printf '%s' "没有可交互的 TTY。非交互安装请设置 NEOGATE_API_KEY。" ;;
      no_tty_client) printf '%s' "没有可交互的 TTY。非交互安装请设置 NEOGATE_CLIENT=codex 或 NEOGATE_CLIENT=claude。" ;;
      no_tty_model) printf '%s' "没有可交互的 TTY，无法选择模型。请在交互式终端中重新运行。" ;;
      read_api_key_failed) printf '%s' "读取 API 密钥失败" ;;
      empty_api_key) printf '%s' "API 密钥不能为空" ;;
      read_client_failed) printf '%s' "读取客户端选择失败" ;;
      read_model_failed) printf '%s' "读取模型选择失败" ;;
      no_tty_yes) printf '%s' "没有可交互的 TTY。请用 --yes 或 NEOGATE_ASSUME_YES=1 重新运行。" ;;
      elevated_missing) printf '%s' "此步骤需要管理员权限，但 sudo 不可用。" ;;
      key_verified) printf '%s' "API 密钥验证成功" ;;
      key_rejected) printf '%s' "API 密钥被拒绝" ;;
      reenter_api_key) printf '%s' "请重新输入 API 密钥。" ;;
      key_loaded) printf '%s' "已从本地配置读取 API 密钥" ;;
      model_current_label) printf '%s' "（当前）" ;;
      client_inferred) printf '使用已配置的客户端：%s' "$1" ;;
      verify_not_found) printf '%s' "找不到验证接口。请确认 NeoGate 已更新且 BASE_URL 正确。" ;;
      connect_failed) printf '无法连接到 %s' "$1" ;;
      verify_failed) printf 'API 密钥验证失败，HTTP %s' "$1" ;;
      node_found) printf 'Node.js %s' "$1" ;;
      npm_found) printf 'npm %s' "$1" ;;
      node_outdated) printf 'Node.js %s 版本过低（需要 >=%s），需要升级。' "$1" "$2" ;;
      node_upgrade_prompt) printf '%s' "是否现在安装 Node.js 22？" ;;
      node_installing_nodesource) printf '%s' "正在通过 NodeSource 仓库安装 Node.js 22..." ;;
      node_missing_disabled) printf '%s' "缺少 Node.js/npm，且安装已禁用。" ;;
      node_missing_prompt) printf '%s' "缺少 Node.js/npm。现在安装吗？" ;;
      node_required) printf '%s' "需要 Node.js/npm。" ;;
      homebrew_required) printf '%s' "macOS 自动安装 Node.js 需要 Homebrew。" ;;
      unsupported_pkg) printf '%s' "不支持当前 Linux 包管理器。请安装 Node.js 和 npm 后重新运行此脚本。" ;;
      unsupported_os) printf '不支持的操作系统：%s' "$1" ;;
      unsupported_arch) printf '不支持的 CPU 架构：%s' "$1" ;;
      node_installing_pkg) printf '正在使用 %s 安装 Node.js（系统目录）...' "$1" ;;
      node_path_missing) printf '%s' "Node.js 安装后未在 PATH 中找到 node" ;;
      npm_path_missing) printf '%s' "Node.js 安装后未在 PATH 中找到 npm" ;;
      node_installed) printf 'Node.js %s' "$1" ;;
      codex_found) printf 'Codex CLI %s' "$1" ;;
      codex_missing_disabled) printf '%s' "缺少 Codex CLI，且安装已禁用。" ;;
      codex_missing_prompt) printf '%s' "缺少 Codex CLI。现在用 npm 安装 @openai/codex 吗？" ;;
      codex_required) printf '%s' "需要 Codex CLI。" ;;
      npm_global_failed) printf '%s' "npm 全局安装失败。" ;;
      codex_sudo_prompt) printf '%s' "是否使用 sudo 重试安装 Codex CLI？" ;;
      codex_install_failed) printf '%s' "Codex CLI 安装失败。" ;;
      codex_path_missing) printf '%s' "Codex CLI 安装后未在 PATH 中找到 codex" ;;
      codex_installed) printf 'Codex CLI %s' "$1" ;;
      claude_found) printf 'Claude Code %s' "$1" ;;
      claude_missing_disabled) printf '%s' "缺少 Claude Code，且安装已禁用。" ;;
      claude_missing_prompt) printf '%s' "缺少 Claude Code。现在用 npm 安装 @anthropic-ai/claude-code 吗？" ;;
      claude_required) printf '%s' "需要 Claude Code。" ;;
      claude_sudo_prompt) printf '%s' "是否使用 sudo 重试安装 Claude Code？" ;;
      claude_install_failed) printf '%s' "Claude Code 安装失败。" ;;
      claude_path_missing) printf '%s' "Claude Code 安装后未在 PATH 中找到 claude" ;;
      claude_installed) printf 'Claude Code %s' "$1" ;;
      backup_file) printf '备份文件：%s' "$1" ;;
      dry_run_preview) printf '%s' "试运行：生成的 Codex config.toml / auth.json 预览" ;;
      dry_run_claude_preview) printf '%s' "试运行：生成的 Claude Code 配置预览" ;;
      config_updated) printf '%s' "配置已更新" ;;
      claude_config_updated) printf '%s' "配置已更新" ;;
      relay_skipped) printf '%s' "跳过最终转发测试" ;;
      relay_testing) printf '正在请求 %s/responses' "$1" ;;
      chat_relay_testing) printf '正在请求 %s/chat/completions' "$1" ;;
      claude_relay_testing) printf '正在请求 %s/v1/messages' "$1" ;;
      relay_succeeded) printf '%s' "网关转发测试成功" ;;
      responses_relay_succeeded) printf '%s' "Responses API 转发测试成功" ;;
      chat_relay_succeeded) printf '%s' "Chat Completions 转发测试成功" ;;
      responses_failed_chat_succeeded) printf '%s' "Responses API 测试失败，但 Chat Completions 测试成功。当前 Codex 配置使用 responses 模式，请注意兼容性。" ;;
      both_relay_failed) printf '%s' "Responses API 和 Chat Completions 测试均未通过。" ;;
      relay_rejected) printf '网关转发拒绝了 API 密钥，HTTP %s' "$1" ;;
      relay_failed) printf "测试未通过：HTTP %s。模型 %s 的上游或价格配置可能还没就绪。" "$1" "$2" ;;
      relay_upstream_hint) printf "模型 %s 的上游或价格配置可能还没就绪。" "$1" ;;
      relay_internal_hint) printf "网关返回内部错误。请检查后端日志、模型 '%s' 的价格配置，以及对应上游通道和 Key 是否启用且健康。" "$1" ;;
      relay_response_detail) printf '网关返回：%s' "$1" ;;
      fetching_models) printf '正在获取 %s 可用模型...' "$1" ;;
      models_rejected) printf '模型列表接口拒绝了 API 密钥，HTTP %s' "$1" ;;
      models_not_found) printf '%s' "找不到模型列表接口。请确认 NeoGate 已更新且 BASE_URL 正确。" ;;
      models_failed) printf '获取模型列表失败，HTTP %s' "$1" ;;
      models_empty) printf '%s' "当前没有可用模型。请联系管理员配置可用上游通道和价格。" ;;
      installer_title) printf '%s 安装器' "$1" ;;
      switch_or_reinstall_prompt) printf '请选择 [1]：' ;;
      switch_option) printf '1. 切换模型' ;;
      change_key_option) printf '2. 更换 API Key' ;;
      reinstall_option) printf '3. 重新安装' ;;
      switch_model) printf '切换模型' ;;
      change_api_key) printf '更换 API Key' ;;
      keeping_model) printf '保留当前模型：%s' "$1" ;;
      model_switched) printf '模型已切换为 %s' "$1" ;;
      api_key_changed) printf '%s' "API Key 已更新" ;;
      step_verify_key) printf '%s' "验证 API 密钥" ;;
      step_choose_client) printf '%s' "选择客户端" ;;
      step_choose_model) printf '%s' "选择默认模型" ;;
      step_check_tools) printf '%s' "检查依赖" ;;
      step_write_config) printf '%s' "写入配置" ;;
      step_test_gateway) printf '%s' "测试网关" ;;
      gateway_url) printf '网关基础 URL：%s' "$1" ;;
      config_summary) printf '%s' "配置摘要" ;;
      summary_client_label) printf '%s' "客户端" ;;
      summary_base_url_label) printf '%s' "Base URL" ;;
      summary_model_label) printf '%s' "默认模型" ;;
      summary_config_file_label) printf '%s' "配置文件" ;;
      codex_model) printf 'Codex 模型：%s' "$1" ;;
      claude_model) printf 'Claude Code 模型：%s' "$1" ;;
      api_key_prompt) printf '%s' "请输入 API 密钥：" ;;
      choose_client_codex) printf '%s' "1. Codex CLI" ;;
      choose_client_claude) printf '%s' "2. Claude Code" ;;
      choose_client_prompt) printf '%s' "请选择客户端 [1-2]：" ;;
      choose_model_title) printf '%s' "请选择默认模型（回车使用 1）：" ;;
      choose_model_prompt) printf '%s' "请输入编号" ;;
      invalid_model) printf '不支持的模型选择：%s' "$1" ;;
      install_tools_prompt) printf '%s' "检查依赖？" ;;
      update_config_prompt) printf '%s' "写入配置？" ;;
      install_tools_skipped) printf '%s' "已跳过检查/安装依赖" ;;
      config_skipped) printf '%s' "已跳过配置更新" ;;
      configured) printf '%s' "Codex CLI 已配置完成" ;;
      claude_configured) printf '%s' "Claude Code 已配置完成" ;;
      try_codex) printf '%s' "试试：codex --version" ;;
      try_claude) printf '%s' "试试：claude --version" ;;
      *) printf '%s' "$key" ;;
    esac
    return 0
  fi

  case "$key" in
    client_required) printf '%s' "--client requires codex or claude" ;;
    unknown_option) printf 'Unknown option: %s' "$1" ;;
    invalid_client) printf 'Unsupported client: %s. Choose codex or claude.' "$1" ;;
    cmd_required) printf '%s is required' "$1" ;;
    no_tty_api_key) printf '%s' "No interactive TTY found. Set NEOGATE_API_KEY for non-interactive installs." ;;
    no_tty_client) printf '%s' "No interactive TTY found. Set NEOGATE_CLIENT=codex or NEOGATE_CLIENT=claude for non-interactive installs." ;;
    no_tty_model) printf '%s' "No interactive TTY found, so the installer cannot choose a model. Re-run in an interactive terminal." ;;
    read_api_key_failed) printf '%s' "Failed to read API key" ;;
    empty_api_key) printf '%s' "API key cannot be empty" ;;
    read_client_failed) printf '%s' "Failed to read client selection" ;;
    read_model_failed) printf '%s' "Failed to read model selection" ;;
    no_tty_yes) printf '%s' "No interactive TTY found. Re-run with --yes or NEOGATE_ASSUME_YES=1." ;;
    elevated_missing) printf '%s' "This step needs elevated privileges, but sudo is not available." ;;
    key_verified) printf '%s' "API key verified" ;;
    key_rejected) printf '%s' "API key was rejected" ;;
    reenter_api_key) printf '%s' "Please enter the API key again." ;;
    key_loaded) printf '%s' "Reusing API key from previous config" ;;
    model_current_label) printf '%s' " (current)" ;;
    client_inferred) printf 'Using previously configured client: %s' "$1" ;;
    verify_not_found) printf '%s' "Verification endpoint was not found. Make sure NeoGate is up to date and BASE_URL is correct." ;;
    connect_failed) printf 'Could not connect to %s' "$1" ;;
    verify_failed) printf 'API key verification failed with HTTP %s' "$1" ;;
    node_found) printf 'Node.js %s' "$1" ;;
    npm_found) printf 'npm %s' "$1" ;;
    node_outdated) printf 'Node.js %s is too old (need >=%s). Upgrade required.' "$1" "$2" ;;
    node_upgrade_prompt) printf '%s' "Install Node.js 22 now?" ;;
    node_installing_nodesource) printf '%s' "Installing Node.js 22 via NodeSource repository..." ;;
    node_missing_disabled) printf '%s' "Node.js/npm is missing and installation is disabled." ;;
    node_missing_prompt) printf '%s' "Node.js/npm is missing. Install it now?" ;;
    node_required) printf '%s' "Node.js/npm is required." ;;
    homebrew_required) printf '%s' "Homebrew is required to install Node.js automatically on macOS." ;;
    unsupported_pkg) printf '%s' "Unsupported Linux package manager. Install Node.js and npm, then re-run this script." ;;
    unsupported_os) printf 'Unsupported operating system: %s' "$1" ;;
    unsupported_arch) printf 'Unsupported CPU architecture: %s' "$1" ;;
    node_installing_pkg) printf 'Installing Node.js system-wide with %s...' "$1" ;;
    node_path_missing) printf '%s' "Node.js installation did not put node on PATH" ;;
    npm_path_missing) printf '%s' "Node.js installation did not put npm on PATH" ;;
    node_installed) printf 'Node.js %s' "$1" ;;
    codex_found) printf 'Codex CLI %s' "$1" ;;
    codex_missing_disabled) printf '%s' "Codex CLI is missing and installation is disabled." ;;
    codex_missing_prompt) printf '%s' "Codex CLI is missing. Install @openai/codex with npm now?" ;;
    codex_required) printf '%s' "Codex CLI is required." ;;
    npm_global_failed) printf '%s' "npm global install failed." ;;
    codex_sudo_prompt) printf '%s' "Retry Codex CLI installation with sudo?" ;;
    codex_install_failed) printf '%s' "Codex CLI installation failed." ;;
    codex_path_missing) printf '%s' "Codex CLI installation did not put codex on PATH" ;;
    codex_installed) printf 'Codex CLI %s' "$1" ;;
    claude_found) printf 'Claude Code %s' "$1" ;;
    claude_missing_disabled) printf '%s' "Claude Code is missing and installation is disabled." ;;
    claude_missing_prompt) printf '%s' "Claude Code is missing. Install @anthropic-ai/claude-code with npm now?" ;;
    claude_required) printf '%s' "Claude Code is required." ;;
    claude_sudo_prompt) printf '%s' "Retry Claude Code installation with sudo?" ;;
    claude_install_failed) printf '%s' "Claude Code installation failed." ;;
    claude_path_missing) printf '%s' "Claude Code installation did not put claude on PATH" ;;
    claude_installed) printf 'Claude Code %s' "$1" ;;
    backup_file) printf 'Backup file: %s' "$1" ;;
    dry_run_preview) printf '%s' "Dry run: generated Codex config.toml / auth.json preview" ;;
    dry_run_claude_preview) printf '%s' "Dry run: generated Claude Code config preview" ;;
    config_updated) printf '%s' "Config updated" ;;
    claude_config_updated) printf '%s' "Config updated" ;;
    relay_skipped) printf '%s' "Skipping final relay test" ;;
    relay_testing) printf 'Requesting %s/responses' "$1" ;;
    chat_relay_testing) printf 'Requesting %s/chat/completions' "$1" ;;
    claude_relay_testing) printf 'Requesting %s/v1/messages' "$1" ;;
    relay_succeeded) printf '%s' "Gateway relay test succeeded" ;;
    responses_relay_succeeded) printf '%s' "Responses API relay test succeeded" ;;
    chat_relay_succeeded) printf '%s' "Chat Completions relay test succeeded" ;;
    responses_failed_chat_succeeded) printf '%s' "Responses API test failed, but Chat Completions succeeded. The current Codex config uses responses mode — check compatibility." ;;
    both_relay_failed) printf '%s' "Both Responses API and Chat Completions tests failed." ;;
    relay_rejected) printf 'Gateway relay rejected the API key with HTTP %s' "$1" ;;
    relay_failed) printf "Test failed: HTTP %s. The upstream or price config for model %s may not be ready." "$1" "$2" ;;
    relay_upstream_hint) printf "The upstream or price config for model %s may not be ready." "$1" ;;
    relay_internal_hint) printf "The gateway returned an internal error. Check backend logs, price config for model '%s', and whether the matching upstream channel/key is enabled and healthy." "$1" ;;
    relay_response_detail) printf 'Gateway response: %s' "$1" ;;
    fetching_models) printf 'Fetching %s models...' "$1" ;;
    models_rejected) printf 'Model list endpoint rejected the API key with HTTP %s' "$1" ;;
    models_not_found) printf '%s' "Model list endpoint was not found. Make sure NeoGate is up to date and BASE_URL is correct." ;;
    models_failed) printf 'Failed to fetch model list with HTTP %s' "$1" ;;
    models_empty) printf '%s' "No models are available. Ask an admin to configure an available upstream channel and price." ;;
    installer_title) printf '%s installer' "$1" ;;
    switch_or_reinstall_prompt) printf 'Choose [1]: ' ;;
    switch_option) printf '1. Switch model' ;;
    change_key_option) printf '2. Change API key' ;;
    reinstall_option) printf '3. Reinstall' ;;
    switch_model) printf 'Switch model' ;;
    change_api_key) printf 'Change API key' ;;
    keeping_model) printf 'Keeping current model: %s' "$1" ;;
    model_switched) printf 'Model switched to %s' "$1" ;;
    api_key_changed) printf '%s' "API key updated" ;;
    step_verify_key) printf '%s' "Verify API key" ;;
    step_choose_client) printf '%s' "Choose client" ;;
    step_choose_model) printf '%s' "Choose default model" ;;
    step_check_tools) printf '%s' "Check dependencies" ;;
    step_write_config) printf '%s' "Write config" ;;
    step_test_gateway) printf '%s' "Test gateway" ;;
    gateway_url) printf 'Gateway base URL: %s' "$1" ;;
    config_summary) printf '%s' "Config summary" ;;
    summary_client_label) printf '%s' "Client" ;;
    summary_base_url_label) printf '%s' "Base URL" ;;
    summary_model_label) printf '%s' "Default model" ;;
    summary_config_file_label) printf '%s' "Config file" ;;
    codex_model) printf 'Codex model: %s' "$1" ;;
    claude_model) printf 'Claude Code model: %s' "$1" ;;
    api_key_prompt) printf '%s' "Enter API key: " ;;
    choose_client_codex) printf '%s' "1. Codex CLI" ;;
    choose_client_claude) printf '%s' "2. Claude Code" ;;
    choose_client_prompt) printf '%s' "Choose client [1-2]: " ;;
    choose_model_title) printf '%s' "Choose the default model (press Enter for 1):" ;;
    choose_model_prompt) printf '%s' "Enter number" ;;
    invalid_model) printf 'Unsupported model selection: %s' "$1" ;;
    install_tools_prompt) printf '%s' "Check dependencies?" ;;
    update_config_prompt) printf '%s' "Write config?" ;;
    install_tools_skipped) printf '%s' "Skipped dependency check/install" ;;
    config_skipped) printf '%s' "Skipped config update" ;;
    configured) printf '%s' "Codex CLI configured" ;;
    claude_configured) printf '%s' "Claude Code configured" ;;
    try_codex) printf '%s' "Try: codex --version" ;;
    try_claude) printf '%s' "Try: claude --version" ;;
    *) printf '%s' "$key" ;;
  esac
}

usage() {
  if [[ "$INSTALL_LANG" == "zh" ]]; then
    cat <<'USAGE'
NeoGate 安装器

用法：
  curl -fsSL __NEOGATE_INSTALL_ORIGIN__/install | bash

选项：
  --client CLIENT      跳过菜单，直接配置 codex 或 claude
  --yes                API 密钥验证后不再询问确认，直接继续
  -h, --help           显示此帮助

环境变量：
  NEOGATE_API_KEY           邮件中获取的 NeoGate API 密钥，适合非交互 shell
  NEOGATE_CLIENT            跳过菜单，直接配置 codex 或 claude
  NEOGATE_LOCALE            强制安装器语言，可设为 zh-CN 或 en-US
  NEOGATE_ASSUME_YES=1      等同于 --yes
  CODEX_HOME                 Codex 配置目录。默认：~/.codex
  CLAUDE_HOME                Claude Code 配置目录。默认：~/.claude
USAGE
    return 0
  fi

  cat <<'USAGE'
NeoGate installer

Usage:
  curl -fsSL __NEOGATE_INSTALL_ORIGIN__/install | bash

Options:
  --client CLIENT      Skip the menu and configure codex or claude
  --yes                Continue without confirmation prompts after API key verification
  -h, --help           Show this help

Environment:
  NEOGATE_API_KEY           NeoGate API key from your email, useful for non-interactive shells
  NEOGATE_CLIENT            Skip the menu and configure codex or claude
  NEOGATE_LOCALE            Force installer language, such as zh-CN or en-US
  NEOGATE_ASSUME_YES=1      Same as --yes
  CODEX_HOME                 Codex config directory. Default: ~/.codex
  CLAUDE_HOME                Claude Code config directory. Default: ~/.claude
USAGE
}

parse_args() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --client)
        [[ $# -ge 2 ]] || die "$(message client_required)"
        CLIENT="$2"
        CLIENT_EXPLICIT=1
        shift 2
        ;;
      --yes|-y)
        ASSUME_YES=1
        shift
        ;;
      -h|--help)
        usage
        exit 0
        ;;
      *)
        die "$(message unknown_option "$1")"
        ;;
    esac
  done
}

have_cmd() {
  command -v "$1" >/dev/null 2>&1
}

need_cmd() {
  have_cmd "$1" || die "$(message cmd_required "$1")"
}

create_tmpdir() {
  TMP_DIR="$(mktemp -d)"
  trap 'rm -rf "$TMP_DIR"' EXIT
}

has_tty() {
  { [[ -t 1 ]] || [[ -t 2 ]]; } && [[ -r /dev/tty && -w /dev/tty ]]
}

prompt_secret() {
  local prompt="$1"
  local force="${2:-0}"
  local value

  if [[ "$force" != "1" && -n "$API_KEY" ]]; then
    return 0
  fi

  has_tty || die "$(message no_tty_api_key)"

  while true; do
    printf '%s' "$prompt" >/dev/tty
    stty -echo </dev/tty
    IFS= read -r value </dev/tty || {
      stty echo </dev/tty
      die "$(message read_api_key_failed)"
    }
    stty echo </dev/tty
    printf '\n' >/dev/tty

    if [[ -n "$value" ]]; then
      API_KEY="$value"
      return 0
    fi

    warn "$(message empty_api_key)"
  done
}

confirm_default_yes() {
  local prompt="$1"
  local answer

  if [[ "$ASSUME_YES" == "1" ]]; then
    return 0
  fi

  has_tty || die "$(message no_tty_yes)"
  printf '%s [Y/n] ' "$prompt" >/dev/tty
  IFS= read -r answer </dev/tty || return 1
  case "$answer" in
    n|N|no|NO|No) return 1 ;;
    *) return 0 ;;
  esac
}
