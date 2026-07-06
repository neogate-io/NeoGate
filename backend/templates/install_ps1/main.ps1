$InstallStep = 0
$InstallTotalSteps = 6

try {
  if ($BaseUrl -eq '__NEOGATE_' + 'DEFAULT_BASE_URL__') {
    Fail (Get-Message not_generated)
  }

  Normalize-BaseUrl
  Load-ExistingCredentials

  Write-Host "$AppName $(Get-Message installer_title_win)"

  Step (Get-Message step_choose_client)
  Select-Client
  Success (Selected-ClientName)

  Step (Get-Message step_verify_key)
  Use-ApiKeyForSelectedClient

  if ($ApiKey -and (Verify-ApiKey)) {
    if (-not $Yes) {
      switch (Choose-SwitchModel) {
        'switch_model' {
          Invoke-SwitchModelFlow
          return
        }
        'change_key' {
          Invoke-ChangeKeyFlow
          return
        }
        'reinstall' {}
      }
    }
  } else {
    if ($ApiKey) { Warn (Get-Message reenter_api_key) }
    $script:ApiKey = $null
    Read-AndVerifyApiKey -ForcePrompt
  }

  Invoke-FullFlow
} catch {
  $message = $_.Exception.Message
  if (-not $message) { $message = "$_" }
  Write-Host $message -ForegroundColor Red
  $global:LASTEXITCODE = 1
  return
}
