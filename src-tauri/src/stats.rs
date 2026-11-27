// Live system stats: CPU, RAM, network (sysinfo) and NVIDIA GPU (nvidia-smi, if present).
use serde::Serialize;
use std::{
    io::Read,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use sysinfo::{Networks, System};

#[derive(Serialize, Clone)]
pub struct Stats { pub cpu: f32, pub ram: f32, pub gpu: f32, pub down: f64, pub up: f64 }

// gpu < 0 means "not available"
static STATS: Mutex<Stats> = Mutex::new(Stats { cpu: 0.0, ram: 0.0, gpu: -1.0, down: 0.0, up: 0.0 });

// The Stats page is the only consumer and it polls once a second while open. Sampling only while
// someone is watching stops nvidia-smi from being spawned every 1.5 s all day and lets the
// CPU/RAM/net sampler skip its refresh when the page is closed.
static WATCH_UNTIL: AtomicU64 = AtomicU64::new(0);

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}
fn watched() -> bool { WATCH_UNTIL.load(Ordering::Relaxed) >= now_secs() }

fn watch() {
    let now = now_secs();
    let prev = WATCH_UNTIL.swap(now + 5, Ordering::Relaxed);
    if prev < now { logf!("stats", "samplers active (stats page open)"); }
}

/// Resolve the real nvidia-smi instead of letting `Command` search the current directory first
/// (a user-writable directory earlier in the search order is a code-execution path).
fn nvidia_smi() -> Option<PathBuf> {
    [
        std::env::var_os("ProgramFiles").map(|d| PathBuf::from(d).join("NVIDIA Corporation").join("NVSMI").join("nvidia-smi.exe")),
        std::env::var_os("SystemRoot").map(|d| PathBuf::from(d).join("System32").join("nvidia-smi.exe")),
    ]
    .into_iter()
    .flatten()
    .find(|p| p.is_file())
}

fn gpu_util() -> Option<f32> {
    let mut c = Command::new(nvidia_smi()?);
    c.args(["--query-gpu=utilization.gpu", "--format=csv,noheader,nounits"]);
    #[cfg(windows)]
    { use std::os::windows::process::CommandExt; c.creation_flags(0x0800_0000); } // no console window flash
    c.stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = c.spawn().ok()?;
    // A wedged driver must not block the only GPU sampler forever: kill the child after 2 s.
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() { return None; }
                let mut out = String::new();
                child.stdout.take()?.read_to_string(&mut out).ok()?;
                return out.lines().next()?.trim().parse().ok();
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            _ => {
                logf!("stats", "nvidia-smi did not answer within 2 s, killing it");
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

pub fn start() {
    logf!("stats", "samplers starting (cpu/ram/net 1s, gpu 1.5s, only while the stats page is open)");
    // One short burst at boot fills the cache, so the Stats page never opens on zeros while the
    // on-demand sample catches up.
    watch();
    thread::spawn(|| {
        let mut sys = System::new();
        let mut nets = Networks::new_with_refreshed_list();
        sys.refresh_cpu_usage();
        let mut active = false;
        // Totals plus a timestamp give a true per-second rate. `received()` is a delta since the
        // last refresh, so after the page was closed it covered the whole idle gap and the first
        // frame showed hundreds of MB/s.
        let mut prev: Option<(u64, u64, Instant)> = None;
        loop {
            thread::sleep(Duration::from_millis(1000));
            if !watched() {
                active = false;
                prev = None;
                continue;
            }
            if !active {
                // First sample after a pause would average the whole idle gap; take a fresh
                // baseline now so the value shown is the last second, like during continuous use.
                active = true;
                sys.refresh_cpu_usage();
                thread::sleep(Duration::from_millis(200));
            }
            sys.refresh_cpu_usage();
            sys.refresh_memory();
            nets.refresh();
            let (mut d, mut u) = (0u64, 0u64);
            for (_, n) in &nets { d += n.total_received(); u += n.total_transmitted(); }
            let now = Instant::now();
            let (down, up) = match prev {
                Some((pd, pu, pt)) => {
                    let secs = now.duration_since(pt).as_secs_f64().max(0.001);
                    ((d.saturating_sub(pd)) as f64 / secs, (u.saturating_sub(pu)) as f64 / secs)
                }
                None => (0.0, 0.0),
            };
            prev = Some((d, u, now));
            let total = sys.total_memory().max(1) as f32;
            let mut s = STATS.lock().unwrap();
            s.cpu = sys.global_cpu_info().cpu_usage();
            s.ram = sys.used_memory() as f32 / total * 100.0;
            s.down = down;
            s.up = up;
        }
    });
    thread::spawn(|| {
        let mut known: Option<bool> = None;
        loop {
            if !watched() {
                // idle: no process is spawned while the Stats page is closed
                thread::sleep(Duration::from_millis(500));
                continue;
            }
            match gpu_util() {
                Some(g) => {
                    if known != Some(true) { logf!("stats", "GPU detected through nvidia-smi"); known = Some(true); }
                    STATS.lock().unwrap().gpu = g;
                    thread::sleep(Duration::from_millis(1500));
                }
                None => {
                    if known != Some(false) { logf!("stats", "no GPU through nvidia-smi (showing n/a)"); known = Some(false); }
                    STATS.lock().unwrap().gpu = -1.0;
                    thread::sleep(Duration::from_secs(30));
                }
            }
        }
    });
}

#[tauri::command]
pub fn stats() -> Stats {
    watch();
    STATS.lock().unwrap().clone()
}
