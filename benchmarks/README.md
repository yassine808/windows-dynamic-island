# Benchmarks

CPU and memory cost of the running island, and latency of a dictation session. Both are captured the
same way every time so changes can be compared.

## CPU and memory

### Method

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

### Run

```powershell
# find the pid of the running app
Get-Process islandwin | Select-Object Id

# 30 s idle sample, printed
.\measure.ps1 -RootPid <pid> -Seconds 30

# and appended to cpu-ram.csv
.\measure.ps1 -RootPid <pid> -Seconds 30 -Label idle -Csv
```

## Dictation latency

### Method

`stt.ps1` reads `%LOCALAPPDATA%\islandwin\islandwin.log` and rebuilds every dictation session from its
lifecycle lines, using the log's boot-relative `+ N.NNNs` offsets so boot restarts cannot skew a delta.
One row per press:

| Column | Meaning |
|---|---|
| `engine` | `browser` = WebView2 streaming recognizer, `winrt` = Windows built-in recognizer |
| `start_ms` | press → engine accepted the session (`listening` / `engine started`) |
| `audio_ms` | press → microphone actually open (browser engine only) |
| `first_words_ms` | press → first interim or final words on screen |
| `final_ms` | press → last final transcript |
| `type_ms` | time inside the `SendInput` call that types the phrase |
| `total_ms` | press → last phrase typed |
| `chars` / `finals` | characters and phrases produced |
| `sample` | last recognised text |

`-Csv` appends only sessions that are not already in `stt-latency.csv`, so it can be run repeatedly.

```powershell
# print the last 20 sessions with per-engine medians
.\stt.ps1

# append new sessions to stt-latency.csv
.\stt.ps1 -Csv
```

### Results

| Date | Finding |
|---|---|
| 2026-11-26 | WinRT recognizer needs **857 ms median** (up to 1910 ms) before it listens, and produced 0 characters in 8 logged sessions: press-and-hold felt dead. |
| 2026-11-26 | WebView2 streaming engine is ready in **12–139 ms** and opens the mic in **197–394 ms**, but the speech service is unreachable inside WebView2 (`network` error 359 ms into the session, endpoint `speech.platform.bing.com:443` is up), so it always falls back. |

## Files

| File | Contents |
|---|---|
| `cpu-ram.csv` | one row per CPU/RAM sample, newest appended at the bottom |
| `measure.ps1` | the CPU/RAM sampler |
| `stt-latency.csv` | one row per dictation session |
| `stt.ps1` | the dictation latency parser |

## History

| Date | Change that motivated the sample |
|---|---|
| 2026-11-26 | baseline: 8 ms Rust pointer loop + 60 Hz JS `get_ptr`/`set_hit_rect` polling, WinRT dictation, always-on stats/GPU samplers |
| 2026-11-26 | push pointer events from Rust, no idle JS timers, streaming dictation in the webview |
