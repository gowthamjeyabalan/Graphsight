//! Optional frame profiler: set GRAPHSIGHT_PROF=1 and Graphsight prints average stage times and memory to stderr
//! every 20 frames. Costs nothing when the variable is not set.

use std::sync::OnceLock;
use std::time::Instant;

pub const STAGES: [&str; 6] = ["sim", "input", "build", "paint", "ui", "wall"];

pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("GRAPHSIGHT_PROF").is_some())
}

pub struct Prof {
    acc: [f64; 6],
    frames: u32,
    last_start: Option<Instant>,
}

impl Default for Prof {
    fn default() -> Self {
        Prof { acc: [0.0; 6], frames: 0, last_start: None }
    }
}

impl Prof {
    /// Add `ms` to a stage (index into STAGES).
    pub fn add(&mut self, stage: usize, since: Instant) {
        if enabled() {
            self.acc[stage] += since.elapsed().as_secs_f64() * 1000.0;
        }
    }

    pub fn frame_start(&mut self) {
        if !enabled() {
            return;
        }
        let now = Instant::now();
        if let Some(prev) = self.last_start {
            self.acc[5] += (now - prev).as_secs_f64() * 1000.0;
        }
        self.last_start = Some(now);
    }

    pub fn frame_end(&mut self, nodes: usize, edges: usize, drawn_edges: usize) {
        if !enabled() {
            return;
        }
        self.frames += 1;
        if self.frames >= 20 {
            let f = self.frames as f64;
            let (rss, hwm) = memory_mb();
            let mut line = String::from("PROF");
            for (k, name) in STAGES.iter().enumerate() {
                line += &format!(" {}={:.2}", name, self.acc[k] / f);
            }
            line += &format!(" rss={rss:.0} hwm={hwm:.0} nodes={nodes} edges={edges} drawn_edges={drawn_edges}");
            eprintln!("{line}");
            self.acc = [0.0; 6];
            self.frames = 0;
        }
    }
}

/// Resident memory and its peak, in MB (Linux and Windows).
pub fn memory_mb() -> (f64, f64) {
    #[cfg(target_os = "linux")]
    {
        let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
        let kb = |key: &str| {
            s.lines().find(|l| l.starts_with(key)).and_then(|l| l.split_whitespace().nth(1)).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0) / 1024.0
        };
        (kb("VmRSS:"), kb("VmHWM:"))
    }
    #[cfg(not(target_os = "linux"))]
    {
        (0.0, 0.0)
    }
}
