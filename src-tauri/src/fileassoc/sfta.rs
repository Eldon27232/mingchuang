//! 调用 PS-SFTA 强制锁定 UserChoice (绕过 Windows 的 hash 防篡改)
//!
//! 上一轮自己 port hash 算法 (wordswap_md) 算错了, Win11 拒绝写入并把整个
//! UserChoice 键清掉, 用户原来的关联全丢。教训: 不自己写 hash, 用社区验证过的脚本。
//!
//! 这里 vendor 了 DanysysTeam/PS-SFTA v1.2.0 (MIT, ~23KB 纯 PowerShell), 编译期
//! 通过 include_str! 内联进二进制, 运行时一次性落到 %TEMP%, 用 PowerShell 调
//! Set-FTA 函数。
//!
//! 来源: https://github.com/DanysysTeam/PS-SFTA  (MIT License)
//! 算法: 两遍 Marvin32 + 微软魔法常量 + ProgId LastWriteTime 戳

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const SFTA_PS1: &str = include_str!("../../resources/SFTA.ps1");

/// 落地后的脚本路径, 整个进程生命周期内只解一次
static EXTRACTED: OnceLock<PathBuf> = OnceLock::new();

/// 把内联的 SFTA.ps1 写到 %TEMP%, 返回脚本路径。同进程多次调用只写一次。
fn ensure_extracted() -> Result<&'static Path> {
    if let Some(p) = EXTRACTED.get() {
        return Ok(p);
    }
    // 用内容长度 + 头 16 字节做版本指纹, 升级时自然换文件名
    let fp: u32 = {
        let head = SFTA_PS1.as_bytes().iter().take(16).copied().map(u32::from).sum::<u32>();
        (SFTA_PS1.len() as u32).wrapping_mul(2654435761).wrapping_add(head)
    };
    let path = std::env::temp_dir().join(format!("kuake-fuckyou-sfta-{:08x}.ps1", fp));

    // 已存在且大小一致就复用 (允许并发实例共享)
    let need_write = match std::fs::metadata(&path) {
        Ok(m) if m.len() as usize == SFTA_PS1.len() => false,
        _ => true,
    };
    if need_write {
        std::fs::write(&path, SFTA_PS1)
            .with_context(|| format!("写 SFTA.ps1 到临时目录失败: {}", path.display()))?;
    }
    let leaked: &'static Path = Box::leak(path.into_boxed_path());
    let _ = EXTRACTED.set(leaked.to_path_buf());
    Ok(leaked)
}

/// 强制把扩展名 `ext` (含点, 如 `.mp4`) 的 UserChoice 锁定到 `progid`。
/// 内部调用 PS-SFTA Set-FTA, 成功后回读 UserChoice 验证 ProgId 字段一致才算成功。
pub fn force_set_user_choice(ext: &str, progid: &str) -> Result<()> {
    if !ext.starts_with('.') {
        return Err(anyhow!("ext 必须以 . 开头: {ext}"));
    }
    if progid.contains('\'') || progid.contains('"') || progid.is_empty() {
        return Err(anyhow!("progid 含非法字符或为空: {progid}"));
    }
    if ext.contains('\'') || ext.contains('"') {
        return Err(anyhow!("ext 含非法字符: {ext}"));
    }

    let script = ensure_extracted()?;
    // dot-source 脚本, 然后调 Set-FTA。用单引号包参数避开 $ 展开。
    // 注意: Set-FTA 内部如果 Test-Path $ProgId 命中会改名, 我们的 ProgId
    // 形如 "KuakeFuckyou.PotPlayerMini64", 不是路径, 不会被改。
    //
    // PowerShell 单引号字符串里 "'" 必须写成 "''"。脚本路径用 %TEMP%, 用户名理论
    // 可含单引号 → 转义。ProgId/ext 已在上面 validate 过没有单引号。
    let script_esc = script.display().to_string().replace('\'', "''");
    let ps_cmd = format!(
        ". '{script_esc}'; Set-FTA -ProgId '{progid}' -Extension '{ext}'"
    );

    let out = crate::sys_cmd::cmd("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &ps_cmd,
        ])
        .output()
        .context("启动 powershell 失败")?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        return Err(anyhow!(
            "Set-FTA 退出码 {:?}, stderr={}, stdout={}",
            out.status.code(),
            stderr.trim(),
            stdout.trim()
        ));
    }

    // 立刻回读 UserChoice, 确认 Windows 真的接受了
    verify_user_choice(ext, progid)
        .with_context(|| format!("Set-FTA 报成功但回读校验失败 (ext={ext}, progid={progid})"))?;

    Ok(())
}

fn verify_user_choice(ext: &str, expected_progid: &str) -> Result<()> {
    use windows_registry::CURRENT_USER;
    let path = format!(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FileExts\\{ext}\\UserChoice"
    );
    let key = CURRENT_USER
        .open(&path)
        .map_err(|e| anyhow!("UserChoice 键不存在或无权读: {e}"))?;
    let got: String = key.get_string("ProgId").unwrap_or_default();
    if got != expected_progid {
        return Err(anyhow!(
            "UserChoice.ProgId 期望 {expected_progid}, 实际 {got}"
        ));
    }
    // Hash 字段存在即可, 内容是 Windows 自己算的, 不再二次校验
    let _hash: String = key.get_string("Hash").unwrap_or_default();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_registry::CURRENT_USER;

    /// 真机回归: 用一个废扩展名 .kkrust1 走完整路径, 然后清理。
    /// 默认 ignore — 真改 HKCU, 需要手动 `cargo test -- --ignored sfta_smoke`
    #[test]
    #[ignore]
    fn sfta_smoke() {
        let ext = ".kkrust1";
        let progid = "KuakeFuckyou.RustSmoke";

        // 先建一个最小 ProgId, 否则 Set-FTA 写完默认应用 explorer 不认
        let cls = format!("Software\\Classes\\{progid}");
        let k = CURRENT_USER.create(&cls).expect("create ProgId");
        k.set_string("", "KK Rust Smoke").expect("set name");

        let r = force_set_user_choice(ext, progid);
        let uc_path = format!(
            "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FileExts\\{ext}\\UserChoice"
        );

        // 清理
        let _ = CURRENT_USER.remove_tree(&uc_path);
        let _ = CURRENT_USER.remove_tree(&format!(
            "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FileExts\\{ext}"
        ));
        let _ = CURRENT_USER.remove_tree(&cls);

        r.expect("force_set_user_choice 失败");
    }
}
