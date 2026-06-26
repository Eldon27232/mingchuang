//! 计划任务动作: disable / enable / query-xml
//!
//! 当前用 schtasks.exe 命令行 (简单, 0 外部依赖, 100% 覆盖)。
//! TODO 下一轮换 Win32 Task Scheduler COM (ITaskService) 更优雅。
//!
//! task 路径形如 `\Microsoft\Windows\Foo\Bar` 或 `\Foo`。

use anyhow::{anyhow, Context, Result};
use std::process::Command;

/// 查询任务的完整 XML 定义 (用于快照)
pub fn query_xml(task_path: &str) -> Result<String> {
    let out = Command::new("schtasks")
        .args(["/Query", "/TN", task_path, "/XML"])
        .output()
        .context("调用 schtasks 失败")?;
    if !out.status.success() {
        return Err(anyhow!(
            "schtasks /Query 失败 ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// 禁用一个计划任务
pub fn disable_task(task_path: &str) -> Result<()> {
    let out = Command::new("schtasks")
        .args(["/Change", "/TN", task_path, "/Disable"])
        .output()
        .context("调用 schtasks 失败")?;
    if !out.status.success() {
        return Err(anyhow!(
            "schtasks /Disable 失败 ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}

/// 启用 (rollback 用)
pub fn enable_task(task_path: &str) -> Result<()> {
    let out = Command::new("schtasks")
        .args(["/Change", "/TN", task_path, "/Enable"])
        .output()
        .context("调用 schtasks 失败")?;
    if !out.status.success() {
        return Err(anyhow!(
            "schtasks /Enable 失败 ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}

/// 查询任务是否存在
pub fn task_exists(task_path: &str) -> bool {
    Command::new("schtasks")
        .args(["/Query", "/TN", task_path])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
