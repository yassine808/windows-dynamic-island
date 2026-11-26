# Benchmarks

CPU and memory cost of the running island, captured the same way every time so changes can be compared.

## Method

`measure.ps1` walks the process tree rooted at the island's main process (the app plus its WebView2
browser, GPU, renderer and utility processes), samples `CPU` (total processor seconds) for `-Seconds`,
and reports:

- `cpu_pct_one_core`: CPU seconds used during the window / wall seconds * 100, i.e. percent of **one**
  core. 100% = one core fully busy; a 16-core machine at 6% of one core is 0.4% of the machine.
- `ram_mb`: summed working set of the tree at the end of the window.
- per-process rows, so a hot renderer or GPU process is visible.

Samples are taken with the island untouched (mouse away, nothing playing) unless the label says
otherwise. Numbers from a `tauri dev` (debug) build are heavier than a `--release` build; the `build`
column records which one was sampled.

## Run

```powershell
# find the pid of the running app
Get-Process islandwin | Select-Object Id

# 30 s idle sample, printed
.\measure.ps1 -RootPid <pid> -Seconds 30

# and appended to cpu-ram.csv
.\measure.ps1 -RootPid <pid> -Seconds 30 -Label idle -Csv
```

## Files

| File | Contents |
|---|---|
| `cpu-ram.csv` | one row per sample, newest appended at the bottom |
| `measure.ps1` | the sampler |

## History

| Date | Change that motivated the sample |
|---|---|
| 2026-11-26 | baseline: 8 ms Rust pointer loop + 60 Hz JS `get_ptr`/`set_hit_rect` polling, WinRT dictation, always-on stats/GPU samplers |
| 2026-11-26 | push pointer events from Rust, no idle JS timers, streaming dictation in the webview |
