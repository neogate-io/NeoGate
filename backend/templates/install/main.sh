main() {
  detect_locale
  parse_args "$@"

  local default_base_url_placeholder="__NEOGATE_""DEFAULT_BASE_URL__"
  if [[ "$BASE_URL" == "$default_base_url_placeholder" ]]; then
    die "install script was not dynamically generated. Use the install command from the NeoGate page."
  fi

  need_cmd curl
  need_cmd awk
  need_cmd sed
  need_cmd mktemp
  create_tmpdir
  normalize_base_url

  load_existing_credentials

  log "$(message installer_title "$APP_NAME")"

  step "$(message step_choose_client)"
  select_client
  success "$(selected_client_name)"

  step "$(message step_verify_key)"
  use_api_key_for_selected_client

  if [[ -n "$API_KEY" ]] && verify_api_key; then
    if [[ "$ASSUME_YES" == "0" ]]; then
      choose_existing_config_action
      case "$EXISTING_CONFIG_ACTION" in
        switch_model)
          run_switch_model_flow
          return 0
          ;;
        change_key)
          run_change_key_flow
          return 0
          ;;
        reinstall)
          ;;
      esac
    fi
  else
    [[ -n "$API_KEY" ]] && warn "$(message reenter_api_key)"
    API_KEY=""
    prompt_and_verify_api_key 1
  fi

  run_full_flow
}

choose_existing_config_action() {
  local answer
  has_tty || {
    EXISTING_CONFIG_ACTION="reinstall"
    return 0
  }
  printf '%s\n' "$(message switch_option)" >/dev/tty
  printf '%s\n' "$(message change_key_option)" >/dev/tty
  printf '%s\n' "$(message reinstall_option)" >/dev/tty
  printf '%s' "$(message switch_or_reinstall_prompt)" >/dev/tty
  IFS= read -r answer </dev/tty || {
    EXISTING_CONFIG_ACTION="reinstall"
    return 0
  }
  # Default (empty) and "1" => switch model; "2" => change API key; "3" => reinstall.
  case "$answer" in
    ''|1)
      EXISTING_CONFIG_ACTION="switch_model"
      ;;
    2)
      EXISTING_CONFIG_ACTION="change_key"
      ;;
    3)
      EXISTING_CONFIG_ACTION="reinstall"
      ;;
    *)
      EXISTING_CONFIG_ACTION="switch_model"
      ;;
  esac
}

run_switch_model_flow() {
  step "$(message switch_model)"
  select_model

  step "$(message step_write_config)"
  case "$CLIENT" in
    codex)
      write_codex_config
      step "$(message step_test_gateway)"
      test_gateway_relay
      ;;
    claude)
      write_claude_config
      step "$(message step_test_gateway)"
      test_claude_gateway_relay
      ;;
  esac

  success "$(message model_switched "$(selected_client_model)")"
}

run_change_key_flow() {
  step "$(message change_api_key)"
  API_KEY=""
  prompt_and_verify_api_key 1

  if ! keep_loaded_model_for_selected_client; then
    step "$(message step_choose_model)"
    select_model
  fi

  step "$(message step_write_config)"
  case "$CLIENT" in
    codex)
      write_codex_config
      step "$(message step_test_gateway)"
      test_gateway_relay
      ;;
    claude)
      write_claude_config
      step "$(message step_test_gateway)"
      test_claude_gateway_relay
      ;;
  esac

  success "$(message api_key_changed)"
}

run_full_flow() {
  local client_name

  client_name="$(selected_client_name)"

  step "$(message step_choose_model)"
  select_model

  print_config_summary "$client_name"

  step "$(message step_check_tools)"
  if confirm_default_yes "$(message install_tools_prompt)"; then
    install_node
    case "$CLIENT" in
      codex)
        install_codex_cli
        ;;
      claude)
        install_claude_code
        ;;
    esac
    space
  else
    warn "$(message install_tools_skipped)"
  fi

  step "$(message step_write_config)"
  if confirm_default_yes "$(message update_config_prompt)"; then
    case "$CLIENT" in
      codex)
        write_codex_config
        step "$(message step_test_gateway)"
        test_gateway_relay
        success "$(message configured)"
        ;;
      claude)
        write_claude_config
        step "$(message step_test_gateway)"
        test_claude_gateway_relay
        success "$(message claude_configured)"
        ;;
    esac
  else
    warn "$(message config_skipped)"
  fi
}

main "$@"
