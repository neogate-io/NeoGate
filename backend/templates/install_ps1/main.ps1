$InstallStep = 0
$InstallTotalSteps = 6

try {
  if ($BaseUrl -eq '__NEOGATE_' + 'DEFAULT_BASE_URL__') {
    Fail (Get-Message not_generated)
  }

  Normalize-BaseUrl
  Load-ExistingCredentials

  Write-Host "$AppName $(Get-Message installer_title_win)"

  Step (Get-Message step_verify_key)
  Read-AndVerifyApiKey

  # Existing-config users get a fast path: switch model or full reinstall.
  # Only offered when config was detected and the caller did not already pin
  # behavior via -Yes / explicit client (those mean "just do the full flow").
  if ($HasExistingConfig -and -not $Yes -and -not $Client) {
    if (Choose-SwitchModel) {
      Invoke-SwitchModelFlow
      return
    }
  }

  Invoke-FullFlow
} catch {
  $message = $_.Exception.Message
  if (-not $message) { $message = "$_" }
  Write-Host $message -ForegroundColor Red
  $global:LASTEXITCODE = 1
  return
}
