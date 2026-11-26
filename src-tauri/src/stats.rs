// Live system stats: CPU, RAM, network (sysinfo) and NVIDIA GPU (nvidia-smi, if present).
use serde::Serialize;
use std::{process::Command, sync::Mutex, thread, time::Duration};
use sysinfo::{Networks, System};

#[derive(Serialize, Clone)]
pub struct Stats { pub cpu: f32, pub ram: f32, pub gpu: f32, pub down: f64, pub up: f64 }

// gpu < 0 means "not available"
static STATS: Mutex<Stats> = Mutex::new(Stats { cpu: 0.0, ram: 0.0, gpu: -1.0, down: 0.0, up: 0.0 });

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
    logf!("stats", "samplers starting (cpu/ram/net 1s, gpu 1.5s)");
    thread::spawn(|| {
        let mut sys = System::new();
        let mut nets = Networks::new_with_refreshed_list();
        sys.refresh_cpu_usage();
        loop {
            thread::sleep(Duration::from_millis(1000));
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
pub fn stats() -> Stats { STATS.lock().unwrap().clone() }
