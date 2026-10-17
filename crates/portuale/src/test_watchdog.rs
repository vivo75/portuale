//! Hang watchdog for the unit-test binary (backlog #376).
//!
//! Started before `main` by an `.init_array` entry, so no test has to opt in.
//! Every 30 s it adds up the CPU ticks of this process and all its
//! descendants; if the sum has not moved for `PORTUALE_TEST_WATCHDOG_IDLE_SECS`
//! (default 600) it prints every thread's stack (`eu-stack`, else `gdb`) and
//! the process tree to stderr, then exits 124 so a gate fails loudly instead
//! of sitting at 0 CPU. `PORTUALE_TEST_WATCHDOG_IDLE_SECS=0` disables it.

use std::collections::HashMap;
use std::time::{Duration, Instant};

#[used]
#[unsafe(link_section = ".init_array")]
static START: extern "C" fn() = start;

extern "C" fn start() {
    let idle_limit = std::env::var("PORTUALE_TEST_WATCHDOG_IDLE_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(600);
    if idle_limit == 0 {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("test-watchdog".into())
        .spawn(move || watch(Duration::from_secs(idle_limit)));
}

/// `(pid, ppid, utime + stime)` of every process, from `/proc/*/stat`.
fn process_table() -> Vec<(u32, u32, u64)> {
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    dir.flatten()
        .filter_map(|e| {
            let pid: u32 = e.file_name().to_str()?.parse().ok()?;
            let stat = std::fs::read_to_string(e.path().join("stat")).ok()?;
            // The command name may hold spaces; fields restart after the last ')'.
            let rest: Vec<&str> = stat[stat.rfind(')')? + 2..].split(' ').collect();
            let ppid = rest.get(1)?.parse().ok()?;
            let ticks = rest.get(11)?.parse::<u64>().ok()? + rest.get(12)?.parse::<u64>().ok()?;
            Some((pid, ppid, ticks))
        })
        .collect()
}

/// This process plus its descendants, as `(pid, ticks)`.
fn tree_ticks(me: u32) -> Vec<(u32, u64)> {
    let table = process_table();
    let mut kids: HashMap<u32, Vec<(u32, u64)>> = HashMap::new();
    let mut own = 0;
    for &(pid, ppid, ticks) in &table {
        if pid == me {
            own = ticks;
        }
        kids.entry(ppid).or_default().push((pid, ticks));
    }
    let mut out = vec![(me, own)];
    let mut queue = vec![me];
    while let Some(p) = queue.pop() {
        for &(pid, ticks) in kids.get(&p).into_iter().flatten() {
            out.push((pid, ticks));
            queue.push(pid);
        }
    }
    out
}

fn watch(idle_limit: Duration) {
    let me = std::process::id();
    let mut last_sum = 0u64;
    let mut last_move = Instant::now();
    loop {
        std::thread::sleep(Duration::from_secs(30));
        let sum: u64 = tree_ticks(me).iter().map(|&(_, t)| t).sum();
        if sum != last_sum {
            last_sum = sum;
            last_move = Instant::now();
            continue;
        }
        if last_move.elapsed() >= idle_limit {
            dump(me, last_move.elapsed());
            std::process::exit(124);
        }
    }
}

fn dump(me: u32, idle: Duration) {
    eprintln!(
        "\n=== test watchdog (#376): no CPU used by this process or its children for {}s ===",
        idle.as_secs()
    );
    for (pid, _) in tree_ticks(me) {
        let cmd = std::fs::read_to_string(format!("/proc/{pid}/cmdline"))
            .unwrap_or_default()
            .replace('\0', " ");
        eprintln!("process {pid}: {cmd}");
    }
    let me_s = me.to_string();
    let tool = std::process::Command::new("eu-stack")
        .args(["-p", &me_s])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .or_else(|| {
            std::process::Command::new("gdb")
                .args(["-p", &me_s, "-batch", "-ex", "thread apply all bt 30"])
                .output()
                .ok()
        });
    match tool {
        Some(o) => {
            eprintln!("{}", String::from_utf8_lossy(&o.stdout));
            eprintln!("{}", String::from_utf8_lossy(&o.stderr));
        }
        None => eprintln!("(neither eu-stack nor gdb is available; no stacks)"),
    }
}
