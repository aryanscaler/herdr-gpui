$ErrorActionPreference = 'Stop'
if (-not $env:HC_TERMINAL_ID -or -not $env:HC_TERMINAL_SOCKET) { exit 0 }
$agent = '__AGENT__'
$program = 'sheprd-acp'
# Keep hook input bounded before passing it to the native helper. The helper
# separately enforces the UTF-8 byte limit and validates the producer ancestry.
$buffer = New-Object char[] 65537
$count = 0
while ($count -lt $buffer.Length) {
    $read = [Console]::In.Read($buffer, $count, $buffer.Length - $count)
    if ($read -eq 0) { break }
    $count += $read
}
if ($count -gt 65536) { exit 1 }
$payload = [string]::new($buffer, 0, $count)
$payload | & $program session-report $agent 2>$null | Out-Null
exit $LASTEXITCODE
