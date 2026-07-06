normalize_client() {
  local value
  value="$(printf '%s' "$1" | tr '[:upper:]' '[:lower:]')"
  case "$value" in
    codex|codex-cli|codex_cli)
      printf '%s' "codex"
      ;;
    claude|claude-code|claude_code)
      printf '%s' "claude"
      ;;
    *)
      return 1
      ;;
  esac
}

select_client() {
  local answer normalized

  if [[ -n "$CLIENT" ]]; then
    normalized="$(normalize_client "$CLIENT")" || die "$(message invalid_client "$CLIENT")"
    CLIENT="$normalized"
    return 0
  fi

  has_tty || die "$(message no_tty_client)"

  printf '%s\n' "$(message choose_client_codex)" >/dev/tty
  printf '%s\n' "$(message choose_client_claude)" >/dev/tty
  printf '%s' "$(message choose_client_prompt)" >/dev/tty

  IFS= read -r answer </dev/tty || die "$(message read_client_failed)"
  case "$answer" in
    1|codex|Codex|CODEX)
      CLIENT="codex"
      ;;
    2|claude|Claude|CLAUDE|claude-code|Claude-Code|CLAUDE-CODE)
      CLIENT="claude"
      ;;
    *)
      die "$(message invalid_client "$answer")"
      ;;
  esac
}

run() {
  if [[ "$DRY_RUN" == "1" ]]; then
    printf '+'
    printf ' %q' "$@"
    printf '\n'
    return 0
  fi

  "$@"
}

run_as_root() {
  if [[ "$DRY_RUN" == "1" ]]; then
    run "$@"
    return 0
  fi

  if [[ "${EUID:-$(id -u)}" -eq 0 ]]; then
    "$@"
  elif have_cmd sudo; then
    sudo "$@"
  else
    die "$(message elevated_missing)"
  fi
}

normalize_base_url() {
  BASE_URL="${BASE_URL%/}"
  BASE_URL="${BASE_URL%/anthropic}"
  if [[ "$BASE_URL" != */v1 ]]; then
    BASE_URL="$BASE_URL/v1"
  fi
  API_ROOT="${BASE_URL%/v1}"
  ANTHROPIC_BASE_URL="$API_ROOT/anthropic"
}

selected_client_base_url() {
  case "$CLIENT" in
    claude)
      printf '%s' "$ANTHROPIC_BASE_URL"
      ;;
    *)
      printf '%s' "$BASE_URL"
      ;;
  esac
}

selected_client_protocol_label() {
  case "$CLIENT" in
    claude)
      printf '%s' "Claude/Anthropic"
      ;;
    *)
      printf '%s' "Codex/OpenAI"
      ;;
  esac
}

selected_client_name() {
  case "$CLIENT" in
    claude)
      printf '%s' "Claude Code"
      ;;
    *)
      printf '%s' "Codex CLI"
      ;;
  esac
}

selected_client_model() {
  case "$CLIENT" in
    claude)
      printf '%s' "$CLAUDE_MODEL"
      ;;
    *)
      printf '%s' "$CODEX_MODEL"
      ;;
  esac
}

selected_config_file() {
  case "$CLIENT" in
    claude)
      printf '%s' "${CLAUDE_HOME:-$HOME/.claude}/settings.json"
      ;;
    *)
      printf '%s, %s' "${CODEX_HOME:-$HOME/.codex}/config.toml" "${CODEX_HOME:-$HOME/.codex}/auth.json"
      ;;
  esac
}

# State populated by load_existing_credentials.
LOADED_CODEX_KEY=""
LOADED_CLAUDE_KEY=""
LOADED_CODEX_MODEL=""
LOADED_CLAUDE_MODEL=""
HAS_EXISTING_CONFIG=0
EXISTING_CONFIG_ACTION=""

json_field() {
  # Extract a string field value from JSON on stdin. $1 = field name.
  local field="$1"
  sed -n 's/.*"'"$field"'"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | sed -n '1p'
}

# Reads existing Codex ~/.codex/auth.json + config.toml and Claude ~/.claude/settings.json
# and remembers previously-used API keys/models.
load_existing_credentials() {
  local codex_home claude_home codex_auth codex_config claude_config
  codex_home="${CODEX_HOME:-$HOME/.codex}"
  claude_home="${CLAUDE_HOME:-$HOME/.claude}"
  codex_auth="$codex_home/auth.json"
  codex_config="$codex_home/config.toml"
  claude_config="$claude_home/settings.json"

  if [[ -f "$codex_auth" ]]; then
    LOADED_CODEX_KEY="$(json_field OPENAI_API_KEY <"$codex_auth")"
  fi
  if [[ -f "$claude_config" ]]; then
    LOADED_CLAUDE_KEY="$(node -e '
      try{var s=require("fs").readFileSync(process.argv[1],"utf8");var o=JSON.parse(s.trim()||"{}");var e=o&&o.env&&o.env.ANTHROPIC_AUTH_TOKEN;if(e)process.stdout.write(String(e))}catch(x){}
    ' "$claude_config" 2>/dev/null || true)"
  fi

  if [[ -f "$codex_config" ]]; then
    LOADED_CODEX_MODEL="$(sed -n 's/^[[:space:]]*model[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$codex_config" | sed -n '1p')"
  fi

  if [[ -f "$claude_config" ]]; then
    LOADED_CLAUDE_MODEL="$(node -e '
      try{var s=require("fs").readFileSync(process.argv[1],"utf8");var o=JSON.parse(s.trim()||"{}");var m=o&&o.model;if(m)process.stdout.write(String(m))}catch(x){}
    ' "$claude_config" 2>/dev/null || true)"
  fi

  local codex_present claude_present
  codex_present=0
  claude_present=0
  [[ -n "$LOADED_CODEX_KEY" || -n "$LOADED_CODEX_MODEL" ]] && codex_present=1
  [[ -n "$LOADED_CLAUDE_KEY" || -n "$LOADED_CLAUDE_MODEL" ]] && claude_present=1
  [[ "$codex_present" == "1" || "$claude_present" == "1" ]] && HAS_EXISTING_CONFIG=1
}

loaded_api_key_for_selected_client() {
  case "$CLIENT" in
    claude)
      printf '%s' "$LOADED_CLAUDE_KEY"
      ;;
    *)
      printf '%s' "$LOADED_CODEX_KEY"
      ;;
  esac
}

use_api_key_for_selected_client() {
  local loaded_key

  [[ -n "$API_KEY" ]] && return 0

  loaded_key="$(loaded_api_key_for_selected_client)"
  if [[ -n "$loaded_key" ]]; then
    API_KEY="$loaded_key"
    detail "$(message key_loaded)"
  fi
}

print_config_summary() {
  local client_name="$1"
  space
  log "$(message config_summary)"
  printf '%-10s %s\n' "$(message summary_client_label)" "$client_name"
  printf '%-10s %s\n' "$(message summary_base_url_label)" "$(selected_client_base_url)"
  printf '%-10s %s\n' "$(message summary_model_label)" "$(selected_client_model)"
  printf '%-10s %s\n' "$(message summary_config_file_label)" "$(selected_config_file)"
  space
}

http_status() {
  local body_file="$1"
  local status
  shift
  status="$(curl -s -o "$body_file" -w '%{http_code}' "$@")" || status="000"
  printf '%s' "$status"
}

print_response_hint() {
  local body_file="$1"
  local msg
  msg="$(response_error_message "$body_file")"
  if [[ -n "$msg" ]]; then
    printf '  %s\n' "$msg" >&2
  fi
}

response_error_message() {
  local body_file="$1"
  # Extract the human-readable error message from a JSON response body.
  # Supports both flat {"error": "..."} and nested {"error": {"message": "..."}}.
  local raw
  raw="$(sed -n 's/.*"error"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$body_file" | sed -n '1p')"
  if [[ -n "$raw" ]]; then
    printf '%s' "$raw"
    return
  fi
  sed -n 's/.*"message"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$body_file" | sed -n '1p'
}

print_relay_failure_hint() {
  local body_file="$1"
  local model="$2"
  local error_message

  error_message="$(response_error_message "$body_file")"
  case "$error_message" in
    *"no available"*channel*|*"upstream unavailable"*|*"price is not configured"*)
      :
      ;;
    "internal server error")
      :
      ;;
    "")
      print_response_hint "$body_file"
      ;;
    *)
      detail "$(message relay_response_detail "$error_message")"
      ;;
  esac
}

verify_api_key() {
  local body_file status
  body_file="$TMP_DIR/verify-response.json"
  rm -f "$body_file"

  status="$(
    http_status "$body_file" \
      -H "authorization: Bearer $API_KEY" \
      "$API_ROOT/api/user-key/verify"
  )"

  case "$status" in
    200)
      success "$(message key_verified)"
      ;;
    401|403)
      warn "$(message key_rejected)"
      return 1
      ;;
    404)
      print_response_hint "$body_file"
      die "$(message verify_not_found)"
      ;;
    000)
      die "$(message connect_failed "$API_ROOT")"
      ;;
    *)
      print_response_hint "$body_file"
      die "$(message verify_failed "$status")"
      ;;
  esac
}

prompt_and_verify_api_key() {
  local force="${1:-0}"

  prompt_secret "$(message api_key_prompt)" "$force"

  while ! verify_api_key; do
    has_tty || die "$(message no_tty_api_key)"
    warn "$(message reenter_api_key)"
    API_KEY=""
    prompt_secret "$(message api_key_prompt)" 1
  done
}

loaded_model_for_selected_client() {
  case "$CLIENT" in
    claude)
      printf '%s' "$LOADED_CLAUDE_MODEL"
      ;;
    *)
      printf '%s' "$LOADED_CODEX_MODEL"
      ;;
  esac
}

keep_loaded_model_for_selected_client() {
  local loaded_model
  loaded_model="$(loaded_model_for_selected_client)"
  [[ -n "$loaded_model" ]] || return 1

  case "$CLIENT" in
    claude)
      CLAUDE_MODEL="$loaded_model"
      ;;
    *)
      CODEX_MODEL="$loaded_model"
      ;;
  esac
  detail "$(message keeping_model "$loaded_model")"
}

extract_model_ids() {
  local body_file="$1"
  sed 's/[{}]/\
/g' "$body_file" \
    | sed -n 's/.*"id"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' \
    | awk '!seen[$0]++'
}

fetch_models() {
  local body_file status
  body_file="$TMP_DIR/models-response.json"
  rm -f "$body_file" "$TMP_DIR/models.txt"

  log "$(message fetching_models "$(selected_client_protocol_label)")"
  if [[ "$CLIENT" == "claude" ]]; then
    status="$(
      http_status "$body_file" \
        -H "x-api-key: $API_KEY" \
        -H "anthropic-version: 2023-06-01" \
        "$ANTHROPIC_BASE_URL/v1/messages/models"
    )"
  else
    status="$(
      http_status "$body_file" \
        -H "authorization: Bearer $API_KEY" \
        "$BASE_URL/models"
    )"
  fi

  case "$status" in
    200)
      extract_model_ids "$body_file" >"$TMP_DIR/models.txt"
      [[ -s "$TMP_DIR/models.txt" ]] || die "$(message models_empty)"
      ;;
    401|403)
      print_response_hint "$body_file"
      die "$(message models_rejected "$status")"
      ;;
    404)
      print_response_hint "$body_file"
      die "$(message models_not_found)"
      ;;
    000)
      die "$(message connect_failed "$BASE_URL")"
      ;;
    *)
      print_response_hint "$body_file"
      die "$(message models_failed "$status")"
      ;;
  esac
}

model_is_available() {
  local expected="$1"
  awk -v expected="$expected" '$0 == expected { found = 1 } END { exit found ? 0 : 1 }' "$TMP_DIR/models.txt"
}

select_model() {
  local explicit current_model answer selected_model count
  local -a models

  fetch_models

  if [[ "$CLIENT" == "claude" ]]; then
    explicit="$CLAUDE_MODEL_EXPLICIT"
    current_model="$CLAUDE_MODEL"
  else
    explicit="$CODEX_MODEL_EXPLICIT"
    current_model="$CODEX_MODEL"
  fi

  if [[ "$explicit" == "1" ]]; then
    model_is_available "$current_model" || die "$(message invalid_model "$current_model")"
    return 0
  fi

  has_tty || die "$(message no_tty_model)"

  # Default to the model saved in the existing config, if it is still available.
  local loaded_model default_index idx
  loaded_model=""
  default_index=1
  if [[ "$CLIENT" == "claude" ]]; then
    loaded_model="$LOADED_CLAUDE_MODEL"
  else
    loaded_model="$LOADED_CODEX_MODEL"
  fi

  printf '%s\n' "$(message choose_model_title)" >/dev/tty
  count=0
  idx=0
  while IFS= read -r selected_model; do
    [[ -n "$selected_model" ]] || continue
    models+=("$selected_model")
    count=$((count + 1))
    idx=$((idx + 1))
    if [[ -n "$loaded_model" && "$selected_model" == "$loaded_model" ]]; then
      default_index="$idx"
      printf '%s. %s%s\n' "$count" "$selected_model" "$(message model_current_label)" >/dev/tty
    else
      printf '%s. %s\n' "$count" "$selected_model" >/dev/tty
    fi
  done <"$TMP_DIR/models.txt"

  [[ "$count" -gt 0 ]] || die "$(message models_empty)"
  printf '%s [%s]: ' "$(message choose_model_prompt)" "$default_index" >/dev/tty
  IFS= read -r answer </dev/tty || die "$(message read_model_failed)"

  if [[ -z "$answer" ]]; then
    answer="$default_index"
  fi

  answer="$(printf '%s' "$answer" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"
  case "$answer" in
    ''|*[!0-9]*)
      die "$(message invalid_model "$answer")"
      ;;
  esac

  if [[ "$answer" -lt 1 || "$answer" -gt "$count" ]]; then
    die "$(message invalid_model "$answer")"
  fi

  selected_model="${models[$((answer - 1))]}"
  space
  if [[ "$CLIENT" == "claude" ]]; then
    CLAUDE_MODEL="$selected_model"
  else
    CODEX_MODEL="$selected_model"
  fi
  success "$selected_model"
}

install_node_nodesource() {
  local kind="$1" setup_url setup_script
  if [[ "$kind" == "deb" ]]; then
    setup_url="https://deb.nodesource.com/setup_lts.x"
  else
    setup_url="https://rpm.nodesource.com/setup_lts.x"
  fi
  setup_script="$TMP_DIR/nodesource-setup.sh"
  run curl -fsSL "$setup_url" -o "$setup_script" || die "$(message connect_failed "$setup_url")"
  run_as_root bash "$setup_script"
  if [[ "$kind" == "deb" ]]; then
    run_as_root apt-get install -y nodejs
  elif have_cmd dnf; then
    run_as_root dnf install -y nodejs
  else
    run_as_root yum install -y nodejs
  fi
}

install_node() {
  if have_cmd node && have_cmd npm; then
    detail_ok "$(message node_found "$(node --version)")"
    detail_ok "$(message npm_found "$(npm --version)")"
    return 0
  fi

  [[ "$SKIP_INSTALL" == "0" ]] || die "$(message node_missing_disabled)"
  confirm_default_yes "$(message node_missing_prompt)" || die "$(message node_required)"

  local os
  os="$(uname -s | tr '[:upper:]' '[:lower:]')"
  case "$os" in
    darwin)
      have_cmd brew || die "$(message homebrew_required)"
      run brew install node
      ;;
    linux)
      if have_cmd apt-get; then
        install_node_nodesource "deb"
      elif have_cmd dnf || have_cmd yum; then
        install_node_nodesource "rpm"
      elif have_cmd pacman; then
        run_as_root pacman -Sy --noconfirm nodejs npm
      else
        die "$(message unsupported_pkg)"
      fi
      ;;
    *)
      die "$(message unsupported_os "$os")"
      ;;
  esac

  have_cmd node || die "$(message node_path_missing)"
  have_cmd npm || die "$(message npm_path_missing)"
  detail_ok "$(message node_installed "$(node --version)")"
  detail_ok "$(message npm_found "$(npm --version)")"
}

install_codex_cli() {
  if have_cmd codex; then
    detail_ok "$(message codex_found "$(codex --version 2>/dev/null || printf 'installed')")"
    return 0
  fi

  [[ "$SKIP_INSTALL" == "0" ]] || die "$(message codex_missing_disabled)"
  confirm_default_yes "$(message codex_missing_prompt)" || die "$(message codex_required)"

  if run npm install -g @openai/codex; then
    :
  else
    warn "$(message npm_global_failed)"
    confirm_default_yes "$(message codex_sudo_prompt)" || die "$(message codex_install_failed)"
    run_as_root npm install -g @openai/codex
  fi

  have_cmd codex || die "$(message codex_path_missing)"
  detail_ok "$(message codex_installed "$(codex --version 2>/dev/null || printf 'installed')")"
}

install_claude_code() {
  if have_cmd claude; then
    detail_ok "$(message claude_found "$(claude --version 2>/dev/null || printf 'installed')")"
    return 0
  fi

  [[ "$SKIP_INSTALL" == "0" ]] || die "$(message claude_missing_disabled)"
  confirm_default_yes "$(message claude_missing_prompt)" || die "$(message claude_required)"

  if run npm install -g @anthropic-ai/claude-code; then
    :
  else
    warn "$(message npm_global_failed)"
    confirm_default_yes "$(message claude_sudo_prompt)" || die "$(message claude_install_failed)"
    run_as_root npm install -g @anthropic-ai/claude-code
  fi

  have_cmd claude || die "$(message claude_path_missing)"
  detail_ok "$(message claude_installed "$(claude --version 2>/dev/null || printf 'installed')")"
}

toml_escape() {
  local value="$1"
  value="${value//\\/\\\\}"
  value="${value//\"/\\\"}"
  printf '%s' "$value"
}

write_codex_config() {
  local codex_home config_file timestamp backup_file clean_file next_file
  local escaped_base_url escaped_model escaped_provider_name

  codex_home="${CODEX_HOME:-$HOME/.codex}"
  config_file="$codex_home/config.toml"
  timestamp="$(date +%Y%m%d%H%M%S)"
  clean_file="$(mktemp)"
  next_file="$(mktemp)"

  escaped_base_url="$(toml_escape "$BASE_URL")"
  escaped_model="$(toml_escape "$CODEX_MODEL")"
  escaped_provider_name="$(toml_escape "$PROVIDER_NAME")"

  if [[ "$DRY_RUN" == "0" ]]; then
    mkdir -p "$codex_home"
    chmod 700 "$codex_home"
  else
    run mkdir -p "$codex_home"
  fi

  if [[ -f "$config_file" ]]; then
    backup_file="$config_file.bak-$timestamp"
    run cp -p "$config_file" "$backup_file"
    awk '
      /^\[/ {
        if ($0 == "[model_providers.neogate]" || $0 == "[model_providers.\"neogate\"]") {
          skip = 1
          next
        }
        skip = 0
        in_table = 1
      }
      skip { next }
      !in_table && /^(model|model_provider|openai_base_url|preferred_auth_method|model_reasoning_effort|OPENAI_API_KEY)[[:space:]]*=/ { next }
      { print }
    ' "$config_file" >"$clean_file"
  else
    : >"$clean_file"
  fi

  {
    printf 'model = "%s"\n' "$escaped_model"
    printf 'model_provider = "%s"\n' "$PROVIDER_ID"
    sed '/./,$!d' "$clean_file"
    printf '\n'
    printf '[model_providers.%s]\n' "$PROVIDER_ID"
    printf 'name = "%s"\n' "$escaped_provider_name"
    printf 'base_url = "%s"\n' "$escaped_base_url"
    printf 'wire_api = "responses"\n'
    printf 'requires_openai_auth = false\n'
  } >"$next_file"

  if [[ "$DRY_RUN" == "1" ]]; then
    log "$(message dry_run_preview)"
    printf '# %s\n' "$config_file"
    cat "$next_file"
    printf '\n# %s\n' "$codex_home/auth.json"
    printf '{\n'
    printf '  "OPENAI_API_KEY": "***",\n'
    printf '  "auth_mode": "apikey"\n'
    printf '}\n'
  else
    mv "$next_file" "$config_file"
    chmod 600 "$config_file"
    success "$(message config_updated)"
    [[ -n "${backup_file:-}" ]] && detail "$(message backup_file "$backup_file")"

    local auth_file="$codex_home/auth.json"
    if [[ -f "$auth_file" ]]; then
      run cp -p "$auth_file" "$auth_file.bak-$timestamp"
    fi
    run node -e "
      var fs=require('fs'),f=process.argv[1],k=process.argv[2],a={};
      if(fs.existsSync(f)){try{a=JSON.parse(fs.readFileSync(f,'utf8').trim()||'{}')}catch(e){a={}}}
      if(!a||typeof a!=='object'||Array.isArray(a))a={};
      a.OPENAI_API_KEY=k;
      a.auth_mode='apikey';
      fs.writeFileSync(f,JSON.stringify(a,null,2)+'\n',{mode:0o600});
    " "$auth_file" "$API_KEY"
    chmod 600 "$auth_file"
  fi

  rm -f "$clean_file" "$next_file"
}

write_claude_config() {
  local claude_home config_file timestamp backup_file next_file
  local escaped_api_root escaped_model escaped_api_key

  claude_home="${CLAUDE_HOME:-$HOME/.claude}"
  config_file="$claude_home/settings.json"
  timestamp="$(date +%Y%m%d%H%M%S)"
  next_file="$(mktemp)"

  if [[ "$DRY_RUN" == "1" ]]; then
    escaped_api_root="$(json_escape "$ANTHROPIC_BASE_URL")"
    escaped_model="$(json_escape "$CLAUDE_MODEL")"
    log "$(message dry_run_claude_preview)"
    cat <<JSON
{
  "env": {
    "ANTHROPIC_BASE_URL": "$escaped_api_root",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "$escaped_model",
    "ANTHROPIC_DEFAULT_OPUS_MODEL": "$escaped_model",
    "ANTHROPIC_DEFAULT_SONNET_MODEL": "$escaped_model",
    "ANTHROPIC_MODEL": "$escaped_model",
    "ANTHROPIC_REASONING_MODEL": "$escaped_model",
    "ANTHROPIC_CUSTOM_MODEL_OPTION": "$escaped_model",
    "ANTHROPIC_AUTH_TOKEN": "***"
  }
}
JSON
    rm -f "$next_file"
    return 0
  fi

  mkdir -p "$claude_home"
  chmod 700 "$claude_home"

  if [[ -f "$config_file" ]]; then
    backup_file="$config_file.bak-$timestamp"
    run cp -p "$config_file" "$backup_file"
  fi

  node - "$config_file" "$next_file" "$ANTHROPIC_BASE_URL" "$API_KEY" "$CLAUDE_MODEL" <<'NODE'
const fs = require('fs');

const [configFile, nextFile, apiRoot, apiKey, model] = process.argv.slice(2);
let settings = {};

if (fs.existsSync(configFile)) {
  const raw = fs.readFileSync(configFile, 'utf8').trim();
  if (raw) {
    settings = JSON.parse(raw);
  }
}

if (!settings || typeof settings !== 'object' || Array.isArray(settings)) {
  settings = {};
}

settings.env = {
  ...(settings.env && typeof settings.env === 'object' && !Array.isArray(settings.env) ? settings.env : {}),
  ANTHROPIC_BASE_URL: apiRoot,
  ANTHROPIC_AUTH_TOKEN: apiKey,
  ANTHROPIC_MODEL: model,
  ANTHROPIC_DEFAULT_OPUS_MODEL: model,
  ANTHROPIC_DEFAULT_SONNET_MODEL: model,
  ANTHROPIC_DEFAULT_HAIKU_MODEL: model,
  ANTHROPIC_REASONING_MODEL: model,
  ANTHROPIC_CUSTOM_MODEL_OPTION: model,
};
delete settings.env.ANTHROPIC_API_KEY;

settings.model = model;

fs.writeFileSync(nextFile, `${JSON.stringify(settings, null, 2)}\n`, { mode: 0o600 });
NODE

  mv "$next_file" "$config_file"
  chmod 600 "$config_file"
  success "$(message claude_config_updated)"
  [[ -n "${backup_file:-}" ]] && detail "$(message backup_file "$backup_file")"
  rm -f "$next_file"
}

json_escape() {
  local value="$1"
  value="${value//\\/\\\\}"
  value="${value//\"/\\\"}"
  printf '%s' "$value"
}

test_gateway_relay() {
  local responses_body_file chat_body_file responses_payload chat_payload responses_status chat_status escaped_model

  if [[ "$SKIP_RELAY_TEST" == "1" ]]; then
    warn "$(message relay_skipped)"
    return 0
  fi

  responses_body_file="$TMP_DIR/relay-responses-response.json"
  chat_body_file="$TMP_DIR/relay-chat-response.json"
  rm -f "$responses_body_file" "$chat_body_file"
  escaped_model="$(json_escape "$CODEX_MODEL")"
  responses_payload='{"model":"'"$escaped_model"'","input":"Reply with OK only.","max_output_tokens":16}'
  chat_payload='{"model":"'"$escaped_model"'","messages":[{"role":"user","content":"Reply with OK only."}],"max_tokens":16}'

  log "$(message relay_testing "$BASE_URL")"
  responses_status="$(
    http_status "$responses_body_file" \
      -X POST \
      -H "authorization: Bearer $API_KEY" \
      -H "content-type: application/json" \
      --data "$responses_payload" \
      "$BASE_URL/responses"
  )"

  case "$responses_status" in
    2??)
      success "$(message responses_relay_succeeded)"
      return 0
      ;;
    401|403)
      print_response_hint "$responses_body_file"
      die "$(message relay_rejected "$responses_status")"
      ;;
    000)
      die "$(message connect_failed "$BASE_URL")"
      ;;
  esac

  print_relay_failure_hint "$responses_body_file" "$CODEX_MODEL"
  warn "$(message relay_failed "$responses_status" "$CODEX_MODEL")"

  log "$(message chat_relay_testing "$BASE_URL")"
  chat_status="$(
    http_status "$chat_body_file" \
      -X POST \
      -H "authorization: Bearer $API_KEY" \
      -H "content-type: application/json" \
      --data "$chat_payload" \
      "$BASE_URL/chat/completions"
  )"

  case "$chat_status" in
    2??)
      success "$(message chat_relay_succeeded)"
      warn "$(message responses_failed_chat_succeeded)"
      ;;
    401|403)
      print_response_hint "$chat_body_file"
      die "$(message relay_rejected "$chat_status")"
      ;;
    000)
      die "$(message connect_failed "$BASE_URL")"
      ;;
    *)
      print_relay_failure_hint "$chat_body_file" "$CODEX_MODEL"
      warn "$(message relay_failed "$chat_status" "$CODEX_MODEL")"
      warn "$(message both_relay_failed)"
      ;;
  esac

  return 0
}

test_claude_gateway_relay() {
  local body_file payload status escaped_model

  if [[ "$SKIP_RELAY_TEST" == "1" ]]; then
    warn "$(message relay_skipped)"
    return 0
  fi

  body_file="$TMP_DIR/claude-relay-response.json"
  rm -f "$body_file"
  escaped_model="$(json_escape "$CLAUDE_MODEL")"
  payload='{"model":"'"$escaped_model"'","messages":[{"role":"user","content":"Reply with OK only."}],"max_tokens":16}'

  log "$(message claude_relay_testing "$ANTHROPIC_BASE_URL")"
  status="$(
    http_status "$body_file" \
      -X POST \
      -H "authorization: Bearer $API_KEY" \
      -H "anthropic-version: 2023-06-01" \
      -H "content-type: application/json" \
      --data "$payload" \
      "$ANTHROPIC_BASE_URL/v1/messages"
  )"

  case "$status" in
    2??)
      success "$(message relay_succeeded)"
      ;;
    401|403)
      print_response_hint "$body_file"
      die "$(message relay_rejected "$status")"
      ;;
    000)
      die "$(message connect_failed "$ANTHROPIC_BASE_URL")"
      ;;
    *)
      print_relay_failure_hint "$body_file" "$CLAUDE_MODEL"
      warn "$(message relay_failed "$status" "$CLAUDE_MODEL")"
      return 0
      ;;
  esac
}
