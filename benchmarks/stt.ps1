# Dictation latency benchmark. Reads the app log, reconstructs every dictation session and records how
# long each stage took. See README.md for what the columns mean.
param(
  [string]$Log = $(if (Test-Path (Join-Path $env:APPDATA "Windows Dynamic Island\logs\islandwin.log")) { Join-Path $env:APPDATA "Windows Dynamic Island\logs\islandwin.log" } elseif (Test-Path (Join-Path $env:APPDATA "dev.yassine808.islandwin\logs\islandwin.log")) { Join-Path $env:APPDATA "dev.yassine808.islandwin\logs\islandwin.log" } else { Join-Path $env:LOCALAPPDATA "islandwin\islandwin.log" }),
  [switch]$Csv,
  [int]$Last = 20
)

if (-not (Test-Path $Log)) { throw "log not found: $Log" }

# log line: "03:00:55.385 + 135.090s [js] stt: engine started"
# the "+ N.NNNs" offset counts from app start and resets on every boot, so it is safe for deltas
$line = [regex]'^\s*(?<ts>\d{2}:\d{2}:\d{2}\.\d{3})\s+\+\s*(?<off>\d+(?:\.\d+)?)s\s+\[(?<tag>[^\]]+)\]\s?(?<msg>.*)$'
$date = (Get-Item $Log).LastWriteTime.ToString("yyyy-MM-dd")

$rows = @()
$cur = $null
# transcription model per engine; the app logs its own name, these are only for lines from older builds
$models = @{ browser = "edge-speech-ceto"; winrt = "windows-speech-winrt" }

function New-Session($ts, $engine, $t0) {
  [pscustomobject]@{
    Ts = $ts; Engine = $engine; Model = $models[$engine]; T0 = $t0
    StartAt = $null; AudioAt = $null; FirstAt = $null; FinalAt = $null; TypeAt = $null
    TypeMs = 0.0; Chars = 0; Finals = 0; Sample = ""
  }
}

$flush = {
  if ($null -eq $cur) { return }
  # a press that never reached the engine (queued, cancelled, still booting) is not a measurement
  if ($cur.StartAt -ne $null -or $cur.FirstAt -ne $null -or $cur.Finals -gt 0 -or $cur.TypeMs -gt 0) {
    $ms = { param($t) if ($null -eq $t) { $null } else { [math]::Round(($t - $cur.T0) * 1000) } }
    $script:rows += [pscustomobject]@{
      timestamp      = $cur.Ts
      date           = $script:date
      engine         = $cur.Engine
      model          = $cur.Model
      start_ms       = & $ms $cur.StartAt
      audio_ms       = & $ms $cur.AudioAt
      first_words_ms = & $ms $cur.FirstAt
      final_ms       = & $ms $cur.FinalAt
      type_ms        = if ($cur.TypeMs -gt 0) { [math]::Round($cur.TypeMs, 1) } else { $null }
      total_ms       = & $ms $cur.TypeAt
      chars          = $cur.Chars
      finals         = $cur.Finals
      sample         = $cur.Sample
    }
  }
  $script:cur = $null
}

foreach ($ln in Get-Content $Log) {
  $m = $line.Match($ln)
  if (-not $m.Success) { continue }
  $ts = $m.Groups['ts'].Value
  $off = [double]::Parse($m.Groups['off'].Value, [cultureinfo]::InvariantCulture)
  $tag = $m.Groups['tag'].Value
  $msg = $m.Groups['msg'].Value

  if ($tag -ne "stt" -and -not ($tag -eq "js" -and $msg.StartsWith("stt:"))) { continue }
  $body = if ($tag -eq "js") { $msg.Substring(4).Trim() } else { $msg }

  # a press opens a session; whatever the previous session got is closed as it stands
  if ($body -like "start: browser engine listening") { & $flush; $cur = New-Session $ts "browser" $off; continue }
  if ($body -like "start: spawning the recognition worker*") { & $flush; $cur = New-Session $ts "winrt" $off; continue }
  if ($body -match '^engine -> (browser|winrt).*model=(\S+)$') { $models[$Matches[1]] = $Matches[2]; continue }
  if ($null -eq $cur) { continue }

  if ($body -match '^engine started(?: in \d+ ms)?$') { $cur.StartAt = $off }
  elseif ($body -eq "audio capture open") { $cur.AudioAt = $off }
  elseif ($body -eq "first words") { if ($null -eq $cur.FirstAt) { $cur.FirstAt = $off } }
  elseif ($body -match '^interim "(.*)"$') { if ($null -eq $cur.FirstAt) { $cur.FirstAt = $off }; if ($Matches[1]) { $cur.Sample = $Matches[1] } }
  elseif ($body -match '^final "(.*)"$') {
    if ($null -eq $cur.FirstAt) { $cur.FirstAt = $off }
    $cur.FinalAt = $off; $cur.Finals++; $cur.Sample = $Matches[1]; $cur.Chars += $Matches[1].Length
  }
  elseif ($body -match '^type: (\d+) chars in ([\d.]+) ms') {
    $cur.Chars += [int]$Matches[1]
    $cur.TypeMs += [double]::Parse($Matches[2], [cultureinfo]::InvariantCulture)
    $cur.TypeAt = $off
  }
  elseif ($body -like "run: listening") { if ($null -eq $cur.StartAt) { $cur.StartAt = $off } }
  elseif ($body -match '^hypothesis: "(.*)"$') { if ($null -eq $cur.FirstAt) { $cur.FirstAt = $off }; $cur.Sample = $Matches[1] }
  elseif ($body -match '^result text: "(.*)"$') {
    if ($null -eq $cur.FirstAt) { $cur.FirstAt = $off }
    $cur.FinalAt = $off; $cur.Finals++; $cur.Sample = $Matches[1]
  }
  elseif ($body -match '^typed the phrase as \d+ key events in ([\d.]+) ms') {
    $cur.TypeMs += [double]::Parse($Matches[1], [cultureinfo]::InvariantCulture)
    $cur.TypeAt = $off
  }
  elseif ($body -like "worker: stopped, transcript is*") { & $flush }
}
& $flush

if (-not $rows) { "no dictation sessions found in $Log"; return }

function Median($v) {
  $a = @($v | Where-Object { $_ -ne $null } | Sort-Object)
  if (-not $a.Count) { return $null }
  $i = [int][math]::Floor($a.Count / 2)
  if ($a.Count % 2) { return [math]::Round($a[$i], 1) }
  return [math]::Round(($a[$i - 1] + $a[$i]) / 2, 1)
}

$rows | Select-Object -Last $Last | Format-Table -AutoSize

foreach ($g in $rows | Group-Object engine) {
  $r = $g.Group
  "--- {0} [{3}]: {1} sessions, {2} typed characters ---" -f $g.Name, $r.Count, (($r.chars | Measure-Object -Sum).Sum), (($r.model | Select-Object -Unique) -join ' / ')
  "  engine ready   median {0} ms" -f (Median $r.start_ms)
  "  mic open       median {0} ms" -f (Median $r.audio_ms)
  "  first words    median {0} ms" -f (Median $r.first_words_ms)
  "  press->typed   median {0} ms" -f (Median $r.total_ms)
  "  keystrokes     median {0} ms" -f (Median $r.type_ms)
}

if ($Csv) {
  $path = Join-Path $PSScriptRoot "stt-latency.csv"
  $known = @{}
  if (Test-Path $path) {
    Import-Csv $path | ForEach-Object { $known["$($_.date)|$($_.timestamp)|$($_.engine)"] = $true }
  }
  $new = @($rows | Where-Object { -not $known["$($_.date)|$($_.timestamp)|$($_.engine)"] })
  if ($new.Count) { $new | Export-Csv -Path $path -Append -Encoding utf8; "appended {0} session(s) to {1}" -f $new.Count, $path }
  else { "no new sessions to append" }
}
