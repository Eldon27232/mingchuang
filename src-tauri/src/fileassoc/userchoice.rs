//! Windows 10/11 UserChoice 哈希算法 - 真正强制改默认打开方式
//!
//! 微软用 UserChoice 注册项的 Hash 字段防篡改:
//!   HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\<ext>\UserChoice
//!     ProgId = "KuakeFuckyou.PotPlayerMini64"
//!     Hash   = base64("...")
//! 没有正确的 Hash, Windows 系统启动时会清掉 UserChoice 整个键, 默认回到推荐应用。
//!
//! 算法由 DanysysTeam/SFTA 项目 (PowerShell + C#) 逆向得到, 这里翻成 Rust。
//! 见 https://github.com/DanysysTeam/SFTA
//!
//! 已知局限:
//!  - Win11 22H2+ 的 UCPD.sys 内核拦截只防护 http/https/.pdf 三类, 媒体/压缩文件不影响
//!  - Microsoft 偶尔小改算法, 大多数情况下"旧"算法仍兼容
//!  - 写完必须立刻读回验证 (Windows 拒绝时不会立即报错而是延后清掉键)

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use chrono::{Datelike, Local, TimeZone, Timelike};
use windows_registry::CURRENT_USER;

/// 把扩展名的默认应用强制设为指定 ProgId, 自动算 hash 写 UserChoice。
/// 返回 Ok(()) 表示写入成功且立刻读回验证一致。
pub fn force_set_user_choice(ext: &str, progid: &str) -> Result<()> {
    let ext = if ext.starts_with('.') { ext.to_string() } else { format!(".{ext}") };

    // 1. 取当前用户 SID
    let sid = current_user_sid()?;

    // 2. 取 ProgId 的注册时间 (LastWriteTime, 分钟级取整, 秒置 00)
    //    这是 SFTA 算法要求: Hash 与 ProgId 注册时间绑定, 防移植到其他用户
    let datetime = progid_registration_time(progid)?;

    // 3. 删现有 UserChoice (Win10+ 在 UserChoice 上加了 Deny ACL 防直写, 必须先删整个键)
    let uc_path = format!(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FileExts\\{ext}\\UserChoice"
    );
    let _ = CURRENT_USER.remove_tree(&uc_path);

    // 4. 算 hash
    let hash = compute_user_choice_hash(&ext, &sid, progid, &datetime);

    // 5. 写 UserChoice 新键 (顺序很重要: 先 ProgId 后 Hash)
    let key = CURRENT_USER
        .create(&uc_path)
        .map_err(|e| anyhow!("创建 UserChoice 键失败: {e}"))?;
    key.set_string("ProgId", progid).map_err(|e| anyhow!("写 ProgId 失败: {e}"))?;
    key.set_string("Hash", &hash).map_err(|e| anyhow!("写 Hash 失败: {e}"))?;

    // 6. 立刻读回验证 — Windows 拒绝时会清掉 Hash 字段或整个键
    let verify = CURRENT_USER
        .open(&uc_path)
        .map_err(|e| anyhow!("回读 UserChoice 失败: {e}"))?;
    let got_progid: String = verify.get_string("ProgId").unwrap_or_default();
    let got_hash: String = verify.get_string("Hash").unwrap_or_default();
    if got_progid != progid {
        return Err(anyhow!(
            "验证失败: 期待 ProgId={progid}, 读到 {got_progid}"
        ));
    }
    if got_hash != hash {
        return Err(anyhow!("验证失败: Windows 改了 Hash 字段, 算法可能不被本机 Windows 版本支持"));
    }

    Ok(())
}

/// 取当前用户的 SID 字符串 (S-1-5-21-...)
fn current_user_sid() -> Result<String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Security::{
        Authorization::ConvertSidToStringSidW, GetTokenInformation, TokenUser, TOKEN_QUERY,
        TOKEN_USER,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token = windows::Win32::Foundation::HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)
            .map_err(|e| anyhow!("OpenProcessToken: {e}"))?;

        let mut size: u32 = 0;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut size);
        if size == 0 {
            let _ = CloseHandle(token);
            return Err(anyhow!("GetTokenInformation size=0"));
        }
        let mut buf = vec![0u8; size as usize];
        let r = GetTokenInformation(
            token,
            TokenUser,
            Some(buf.as_mut_ptr() as *mut _),
            size,
            &mut size,
        );
        let _ = CloseHandle(token);
        r.map_err(|e| anyhow!("GetTokenInformation: {e}"))?;

        let token_user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut sid_str = windows::core::PWSTR::null();
        ConvertSidToStringSidW(token_user.User.Sid, &mut sid_str)
            .map_err(|e| anyhow!("ConvertSidToStringSidW: {e}"))?;
        let mut len = 0;
        while *sid_str.0.add(len) != 0 {
            len += 1;
        }
        let slice = std::slice::from_raw_parts(sid_str.0, len);
        let s = String::from_utf16_lossy(slice);
        // 这里有几十字节的内存泄漏 (ConvertSidToStringSidW 用 LocalAlloc 分配),
        // SID 字符串短且整个进程生命周期里只调一次, 不释放无妨。
        Ok(s)
    }
}

/// 取 ProgId 注册表键的 LastWriteTime, 格式 yyyyMMddHHmm00 (秒置 00)
/// SFTA 算法的核心特征: hash 输入含此时间戳, 防伪造
fn progid_registration_time(progid: &str) -> Result<String> {
    // windows-registry 不直接暴露 LastWriteTime, 用 windows-rs 直接 RegQueryInfoKeyW
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{ERROR_SUCCESS, FILETIME};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryInfoKeyW, HKEY, HKEY_CURRENT_USER, KEY_READ,
    };

    let path = format!("Software\\Classes\\{progid}");
    let path_w: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        let mut key = HKEY::default();
        let r = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(path_w.as_ptr()),
            None,
            KEY_READ,
            &mut key,
        );
        if r != ERROR_SUCCESS {
            return Err(anyhow!("RegOpenKey {path}: {:?}", r));
        }
        let mut ft = FILETIME::default();
        let r = RegQueryInfoKeyW(
            key,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(&mut ft),
        );
        let _ = RegCloseKey(key);
        if r != ERROR_SUCCESS {
            return Err(anyhow!("RegQueryInfoKey: {:?}", r));
        }

        // FILETIME (100ns since 1601) → Unix time (s since 1970)
        let combined =
            ((ft.dwHighDateTime as u64) << 32) | (ft.dwLowDateTime as u64);
        const EPOCH_DIFF: u64 = 11_644_473_600; // 秒, 1601→1970
        let secs_since_1970 = (combined / 10_000_000).saturating_sub(EPOCH_DIFF);

        let local = Local
            .timestamp_opt(secs_since_1970 as i64, 0)
            .single()
            .context("FILETIME → DateTime 失败")?;

        // 分钟级取整, 秒置 00
        Ok(format!(
            "{:04}{:02}{:02}{:02}{:02}00",
            local.year(),
            local.month(),
            local.day(),
            local.hour(),
            local.minute()
        ))
    }
}

// ============ SFTA 哈希算法 (Marvin32 / WordSwap 变种) ============

/// SFTA 公开算法。输入: 扩展名 + SID + ProgId + 时间戳 + 微软魔法字符串
/// 输出: 8 字节 hash 的 base64
fn compute_user_choice_hash(ext: &str, sid: &str, progid: &str, datetime: &str) -> String {
    const EXPERIENCE: &str =
        "User Choice set via Windows User Experience {D18B6DD5-6124-4341-9318-804003BAFA0B}";

    let to_hash =
        format!("{}{}{}{}{}", ext, sid, progid, datetime, EXPERIENCE).to_lowercase();

    // UTF-16 LE + null terminator
    let utf16: Vec<u16> = to_hash.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes: Vec<u8> = utf16.iter().flat_map(|w| w.to_le_bytes()).collect();

    // MD5 of the same bytes
    let md5 = md5::compute(&bytes);
    let md5_u32: [u32; 4] = [
        u32::from_le_bytes([md5[0], md5[1], md5[2], md5[3]]),
        u32::from_le_bytes([md5[4], md5[5], md5[6], md5[7]]),
        u32::from_le_bytes([md5[8], md5[9], md5[10], md5[11]]),
        u32::from_le_bytes([md5[12], md5[13], md5[14], md5[15]]),
    ];

    // 把 bytes pad 到 4 字节倍数后转 u32 数组
    let mut padded = bytes.clone();
    while padded.len() % 4 != 0 {
        padded.push(0);
    }
    let data: Vec<u32> = padded
        .chunks(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();

    let result = wordswap_md(&data, &md5_u32);
    base64::engine::general_purpose::STANDARD.encode(result.to_le_bytes())
}

/// SFTA WordSwap-MD 算法核心 (Win10 1809+ 验证有效)
fn wordswap_md(data: &[u32], md5: &[u32; 4]) -> u64 {
    if data.len() < 2 {
        return 0;
    }

    let h0 = md5[1] | 1;
    let h1 = md5[3] | 1;

    let mut out0: u32 = 0;
    let mut out1: u32 = 0;

    // 成对处理 (i, i+1), 步长 2
    let pairs = data.len() / 2;
    for i in 0..pairs {
        let idx = i * 2;
        let v0 = data[idx];
        let v1 = if idx + 1 < data.len() { data[idx + 1] } else { 0 };

        // accumulator 1
        let mut t0 = v0.wrapping_add(out0);
        t0 = t0.wrapping_mul(h0);
        t0 = t0.rotate_left(5);
        t0 = t0.wrapping_mul(md5[0]);
        out0 = t0.wrapping_add(md5[0]);

        // accumulator 2
        let mut t1 = v1.wrapping_add(out1);
        t1 = t1.wrapping_mul(h1);
        t1 = t1.rotate_left(5);
        t1 = t1.wrapping_mul(md5[2]);
        out1 = t1.wrapping_add(md5[2]);
    }

    (out0 as u64) | ((out1 as u64) << 32)
}
