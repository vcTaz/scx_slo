// SPDX-License-Identifier: GPL-2.0-only
//
// scx_slo: user-space sched_ext scheduler built on scx_rustland_core.
// Derived from scx_rlfifo (sched-ext/scx v1.1.4, Andrea Righi).
//
// This version provides the baseline modes the experiments need now:
//
//   scx_slo                      B4-partial: SCHED_EXT tasks only, any CPU
//   scx_slo --cpus 12-19         B5-fixedk:  SCHED_EXT tasks only, confined to 8 CPUs
//   scx_slo --full               every task (not used by the experiments)
//
// The adaptive controller (crates/slo-ctrl) plugs in at `pick_cpu` and
// `slice_ns` in W5, once the latency probe exists.
//
// Prints one CSV line of dispatch statistics per second on stdout. Watch
// `congested`: tasks that overflow the user-space queue go to the shared
// DSQ and can run on any CPU, escaping --cpus (TAXONOMY R-overflow).

mod bpf_skel;
pub use bpf_skel::*;
pub mod bpf_intf;

#[rustfmt::skip]
mod bpf;

use std::mem::MaybeUninit;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use bpf::*;
use clap::Parser;
use libbpf_rs::OpenObject;
use scx_utils::UserExitInfo;
use scx_utils::libbpf_clap_opts::LibbpfOpts;

#[derive(Parser, Debug, Clone)]
#[command(version, about = "scx_slo: SLO-aware sched_ext scheduler (baseline modes)")]
struct Opts {
    /// Schedule every task. Default is partial mode: only tasks whose policy
    /// is SCHED_EXT (start them with tools/ext_exec); the rest stay on EEVDF.
    #[arg(long)]
    full: bool,

    /// CPUs that sched_ext tasks may run on, e.g. "12-19" or "0,2,4-7".
    /// Default: all CPUs.
    #[arg(long)]
    cpus: Option<String>,

    /// Time slice for dispatched tasks, in microseconds.
    #[arg(long, default_value_t = 5000)]
    slice_us: u64,

    /// Let the BPF side dispatch waking tasks straight to idle CPUs without
    /// asking user space. Faster, but bypasses --cpus, so it is refused
    /// together with --cpus.
    #[arg(long)]
    builtin_idle: bool,
}

struct Scheduler<'a> {
    bpf: BpfScheduler<'a>,
    /// allowed[cpu] is true if sched_ext tasks may run there.
    allowed: Vec<bool>,
    allowed_list: Vec<i32>,
    confined: bool,
    next_rr: usize,
    slice_ns: u64,
}

impl<'a> Scheduler<'a> {
    fn init(opts: &Opts, cpus: &[i32], nr_cpus: usize, open_object: &'a mut MaybeUninit<OpenObject>) -> Result<Self> {
        let slice_ns = opts.slice_us * 1000;
        let bpf = BpfScheduler::init(
            open_object,
            LibbpfOpts::default().into_bpf_open_opts(),
            0,                 // exit_dump_len: default
            !opts.full,        // partial: only SCHED_EXT tasks
            false,             // debug
            opts.builtin_idle, // builtin_idle: off so every task goes through pick_cpu
            false,             // numa_local
            slice_ns,          // default slice for tasks the BPF side dispatches itself
            "slo",
        )?;
        let mut allowed = vec![false; nr_cpus];
        for &c in cpus {
            allowed[c as usize] = true;
        }
        Ok(Self {
            bpf,
            confined: cpus.len() < nr_cpus,
            allowed,
            allowed_list: cpus.to_vec(),
            next_rr: 0,
            slice_ns,
        })
    }

    fn is_allowed(&self, cpu: i32) -> bool {
        cpu >= 0 && self.allowed.get(cpu as usize).copied().unwrap_or(false)
    }

    /// Choose the CPU for a task: an idle allowed CPU if there is one, else
    /// the task's previous CPU if allowed (cache-warm), else round-robin
    /// over the allowed set. Unconfined, any idle CPU or the first free one.
    fn pick_cpu(&mut self, task: &QueuedTask) -> i32 {
        let idle = self.bpf.select_cpu(task.pid, task.cpu, task.flags);
        if !self.confined {
            return if idle >= 0 { idle } else { RL_CPU_ANY };
        }
        if self.is_allowed(idle) {
            return idle;
        }
        if self.is_allowed(task.cpu) {
            return task.cpu;
        }
        self.next_rr = (self.next_rr + 1) % self.allowed_list.len();
        self.allowed_list[self.next_rr]
    }

    fn dispatch_tasks(&mut self) {
        while let Ok(Some(task)) = self.bpf.dequeue_task() {
            let mut dispatched = DispatchedTask::new(&task);
            dispatched.cpu = self.pick_cpu(&task);
            dispatched.slice_ns = self.slice_ns;
            if let Err(e) = self.bpf.dispatch_task(&dispatched) {
                eprintln!("dispatch of pid {} failed: {e}", task.pid);
            }
        }
        // Sleeps until more tasks need scheduling.
        self.bpf.notify_complete(0);
    }

    fn print_stats(&mut self) {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
        println!(
            "{now:.3},{},{},{},{},{},{},{},{}",
            *self.bpf.nr_user_dispatches_mut(),
            *self.bpf.nr_kernel_dispatches_mut(),
            *self.bpf.nr_cancel_dispatches_mut(),
            *self.bpf.nr_bounce_dispatches_mut(),
            *self.bpf.nr_failed_dispatches_mut(),
            *self.bpf.nr_sched_congested_mut(),
            *self.bpf.nr_queued_mut(),
            *self.bpf.nr_scheduled_mut(),
        );
    }

    fn run(&mut self) -> Result<UserExitInfo> {
        let mut last = Instant::now();
        while !self.bpf.exited() {
            self.dispatch_tasks();
            if last.elapsed().as_secs() >= 1 {
                self.print_stats();
                last = Instant::now();
            }
        }
        self.bpf.shutdown_and_report()
    }
}

/// Parse "0-3,8,10-11" into sorted, deduplicated CPU ids below `nr_cpus`.
fn parse_cpu_list(s: &str, nr_cpus: usize) -> Result<Vec<i32>> {
    let mut cpus = Vec::new();
    for part in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (lo, hi) = match part.split_once('-') {
            Some((a, b)) => (a.trim().parse::<usize>()?, b.trim().parse::<usize>()?),
            None => {
                let c = part.parse::<usize>().with_context(|| format!("bad CPU '{part}'"))?;
                (c, c)
            }
        };
        if lo > hi || hi >= nr_cpus {
            bail!("CPU range '{part}' is invalid for {nr_cpus} CPUs");
        }
        cpus.extend((lo..=hi).map(|c| c as i32));
    }
    cpus.sort_unstable();
    cpus.dedup();
    if cpus.is_empty() {
        bail!("empty CPU list '{s}'");
    }
    Ok(cpus)
}

fn main() -> Result<()> {
    let opts = Opts::parse();
    if opts.builtin_idle && opts.cpus.is_some() {
        bail!("--builtin-idle lets tasks escape --cpus; use one or the other");
    }
    if opts.slice_us == 0 {
        bail!("--slice-us must be positive");
    }
    let nr_cpus = libbpf_rs::num_possible_cpus().context("counting CPUs")?;
    let cpus = match &opts.cpus {
        Some(s) => parse_cpu_list(s, nr_cpus)?,
        None => (0..nr_cpus as i32).collect(),
    };

    println!(
        "# scx_slo mode={} cpus={} slice_us={} builtin_idle={}",
        if opts.full { "full" } else { "partial" },
        opts.cpus.as_deref().unwrap_or("all"),
        opts.slice_us,
        opts.builtin_idle
    );
    println!("unix_s,user_dispatches,kernel_dispatches,cancel,bounce,failed,congested,queued,scheduled");

    // Everything above runs before init(): init() locks memory and installs
    // a seccomp filter that blocks new mmaps (TAXONOMY R-seccomp).
    let mut open_object = MaybeUninit::uninit();
    loop {
        let mut sched = Scheduler::init(&opts, &cpus, nr_cpus, &mut open_object)?;
        if !sched.run()?.should_restart() {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_cpu_list;

    #[test]
    fn parses_cpu_lists() {
        assert_eq!(parse_cpu_list("0-3", 8).unwrap(), vec![0, 1, 2, 3]);
        assert_eq!(parse_cpu_list("5, 1-2,2", 8).unwrap(), vec![1, 2, 5]);
        assert!(parse_cpu_list("6-9", 8).is_err());
        assert!(parse_cpu_list("3-1", 8).is_err());
        assert!(parse_cpu_list("", 8).is_err());
        assert!(parse_cpu_list("x", 8).is_err());
    }
}
