# Samples CPU (seconds) and memory of an app and its whole process tree.
# See README.md for the method and the meaning of cpu_pct_one_core.
param(
  [Parameter(Mandatory = $true)][int]$RootPid,
  [int]$Seconds = 30,
  [string]$Label = "idle",
  [string]$Build = "dev",
  [switch]$Csv
)

function Get-Tree([int]$p, $all) {
  $ids = @($p)
  $frontier = @($p)
  while ($frontier.Count -gt 0) {
    $next = @()
    foreach ($f in $frontier) { $next += $all | Where-Object { $_.ParentProcessId -eq $f } | Select-Object -ExpandProperty ProcessId }
    $frontier = $next
    $ids += $next
  }
  return $ids
}

$all = Get-CimInstance Win32_Process
$tree = Get-Tree $RootPid $all
$procs = Get-Process -Id $tree -ErrorAction SilentlyContinue
if (-not $procs) { throw "no process with pid $RootPid" }

$before = @{}
foreach ($p in $procs) { $before[$p.Id] = $p.CPU }
$ids = @($procs | Select-Object -ExpandProperty Id)
Start-Sleep -Seconds $Seconds
$after = Get-Process -Id $ids -ErrorAction SilentlyContinue

$rows = @()
$sum = 0.0
foreach ($p in $after) {
  $d = $p.CPU - ($before[$p.Id] ?? $p.CPU)
  $sum += $d
  $rows += [pscustomobject]@{ Id = $p.Id; Name = $p.ProcessName; CpuMs = [math]::Round($d * 1000); RamMB = [math]::Round($p.WorkingSet64 / 1MB, 1) }
}

$ram = [math]::Round((($after | Measure-Object WorkingSet64 -Sum).Sum) / 1MB)
$pct = [math]::Round((($sum / $Seconds) * 100), 1)
$stamp = Get-Date -Format "yyyy-MM-ddTHH:mm:ss"
$row = [pscustomobject]@{
  timestamp     = $stamp
  label         = $Label
  build         = $Build
  seconds       = $Seconds
  cpu_pct_core  = $pct
  cpu_ms_total  = [math]::Round($sum * 1000)
  ram_mb        = $ram
  processes     = $after.Count
  hottest       = (($rows | Sort-Object CpuMs -Descending | Select-Object -First 1) | ForEach-Object { "$($_.Name):$($_.CpuMs)ms" })
}

if ($Csv) { $row | Export-Csv -Path (Join-Path $PSScriptRoot "cpu-ram.csv") -Append -Encoding utf8 }

"--- {0} | {1} | {2}s ---" -f $Label, $Build, $Seconds
$rows | Sort-Object CpuMs -Descending | Format-Table -AutoSize
"{0}% of one core   {1} MB tree RAM   {2} processes" -f $pct, $ram, $after.Count
