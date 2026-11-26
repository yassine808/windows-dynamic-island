// Live system stats: CPU, RAM, network (sysinfo) and NVIDIA GPU (nvidia-smi, if present).
use serde::Serialize;
use std::{
    process::Command,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
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

fn gpu_util() -> Option<f32> {
    let mut c = Command::new("nvidia-smi");
    c.args(["--query-gpu=utilization.gpu", "--format=csv,noheader,nounits"]);
    #[cfg(windows)]
    { use std::os::windows::process::CommandExt; c.creation_flags(0x0800_0000); } // no console window flash
    let o = c.output().ok()?;
    if !o.status.success() { return None; }
    String::from_utf8_lossy(&o.stdout).lines().next()?.trim().parse().ok()
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
        loop {
            thread::sleep(Duration::from_millis(1000));
            if !watched() {
                active = false;
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
            for (_, n) in &nets { d += n.received(); u += n.transmitted(); }
            let total = sys.total_memory().max(1) as f32;
            let mut s = STATS.lock().unwrap();
            s.cpu = sys.global_cpu_info().cpu_usage();
            s.ram = sys.used_memory() as f32 / total * 100.0;
            s.down = d as f64;
            s.up = u as f64;
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
