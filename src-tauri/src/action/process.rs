//! 进程动作: kill (按 exe 名)
//!
//! 不可逆 — rollback 是 null。但执行前会列出所有命中的 (pid, exe path) 写入快照
//! manifest 以便审计。

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sysinfo::System;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KilledProcess {
    pub pid: u32,
    pub name: String,
    pub exe: String,
}

/// 杀掉所有匹配名字的进程。返回 (尝试数, 成功数, 命中列表)。
/// `name` 大小写不敏感比较 (含 .exe 后缀)。
pub fn kill_by_name(name: &str) -> Result<(usize, usize, Vec<KilledProcess>)> {
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let target = name.to_ascii_lowercase();

    let mut killed = Vec::new();
    let mut attempts = 0;
    let mut success = 0;

    for (pid, proc) in sys.processes() {
        let proc_name = proc.name().to_string_lossy().to_ascii_lowercase();
        if proc_name != target {
            continue;
        }
        attempts += 1;
        let exe = proc.exe().map(|p| p.display().to_string()).unwrap_or_default();
        if proc.kill() {
            success += 1;
            killed.push(KilledProcess {
                pid: pid.as_u32(),
                name: proc.name().to_string_lossy().to_string(),
                exe,
            });
        }
    }
    Ok((attempts, success, killed))
}

/// dry-run 预览: 当前会命中多少个进程
pub fn preview_count(name: &str) -> usize {
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let target = name.to_ascii_lowercase();
    sys.processes()
        .values()
        .filter(|p| p.name().to_string_lossy().to_ascii_lowercase() == target)
        .count()
}
