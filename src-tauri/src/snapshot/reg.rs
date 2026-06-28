//! 注册表子树快照与还原
//!
//! 设计:
//! - 序列化格式: 自描述 JSON, 包含 hive + path + values + 递归 subkeys
//! - 二进制数据 (REG_BINARY 等) 用 base64
//! - 还原: 严格按原样写回 (类型/数据), 缺失的中间键自动创建
//!
//! 局限:
//! - SAM/SECURITY 受保护键自然访问不到, 已被 whitelist 拒绝
//! - 不处理 ACL/Audit (P0 暂不需要), 还原后的 ACL 是默认值
//!
//! 注: windows-registry 0.6 提供较高层 API, 但 REG_TYPE 区分有限。
//! 为保证正确性, 本实现先支持最常见的 REG_SZ / REG_EXPAND_SZ / REG_DWORD / REG_QWORD /
//! REG_MULTI_SZ / REG_BINARY, 其他类型回退到 REG_BINARY 透传。

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use windows_registry::{Key, CURRENT_USER, LOCAL_MACHINE, CLASSES_ROOT, USERS, CURRENT_CONFIG};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegSubtree {
    pub hive: String,
    pub path: String,
    #[serde(default)]
    pub values: Vec<RegValue>,
    #[serde(default)]
    pub subkeys: Vec<RegSubtree>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegValue {
    pub name: String,           // 默认值用空串
    pub kind: RegValueKind,
    pub data: String,           // SZ/EXPAND_SZ 原文, DWORD/QWORD 十进制, MULTI_SZ "\0" 连接, BINARY base64
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RegValueKind {
    Sz,
    ExpandSz,
    Dword,
    Qword,
    MultiSz,
    Binary,
}

fn hive_root(hive: &str) -> Result<&'static Key> {
    Ok(match hive {
        "HKCU" => CURRENT_USER,
        "HKLM" => LOCAL_MACHINE,
        "HKCR" => CLASSES_ROOT,
        "HKU"  => USERS,
        "HKCC" => CURRENT_CONFIG,
        other  => return Err(anyhow!("未知 hive: {other}")),
    })
}

/// 递归读取一个子树。如果路径不存在, 返回空树(values+subkeys 都空, 仍记录 hive/path 供 restore 跳过)。
pub fn read_subtree(hive: &str, path: &str) -> Result<RegSubtree> {
    let root = hive_root(hive)?;
    let mut node = RegSubtree {
        hive: hive.to_string(),
        path: path.to_string(),
        values: Vec::new(),
        subkeys: Vec::new(),
    };
    let key = match root.open(path) {
        Ok(k) => k,
        Err(_) => return Ok(node), // 不存在视为空
    };

    // 读所有值
    for v in key.values()? {
        let (name, value) = v;
        let (kind, data) = serialize_value(&value);
        node.values.push(RegValue { name, kind, data });
    }

    // 递归读子键
    for sub_name in key.keys()? {
        let sub_path = if path.is_empty() {
            sub_name.clone()
        } else {
            format!("{path}\\{sub_name}")
        };
        match read_subtree(hive, &sub_path) {
            Ok(child) => node.subkeys.push(child),
            Err(e) => eprintln!("子键读失败 {sub_path}: {e:#}"),
        }
    }
    Ok(node)
}

/// 统一字节级路径: 所有类型都存 (Type tag, base64 字节)。
/// 还原时 set_bytes(name, type, raw_bytes), 100% 字节级精确,
/// 不区分 SZ/EXPAND_SZ/DWORD/QWORD/MULTI_SZ/BINARY 的解码逻辑, 简单可靠。
fn serialize_value(v: &windows_registry::Value) -> (RegValueKind, String) {
    use windows_registry::Type as T;
    let kind = match v.ty() {
        T::String => RegValueKind::Sz,
        T::ExpandString => RegValueKind::ExpandSz,
        T::U32 => RegValueKind::Dword,
        T::U64 => RegValueKind::Qword,
        T::MultiString => RegValueKind::MultiSz,
        _ => RegValueKind::Binary,
    };
    let bytes: &[u8] = v.as_ref();
    let data = base64::engine::general_purpose::STANDARD.encode(bytes);
    (kind, data)
}

/// 把子树原样写回。已存在的会覆盖。
pub fn write_subtree(node: &RegSubtree) -> Result<()> {
    let root = hive_root(&node.hive)?;
    let key = root.create(&node.path).with_context(|| format!("创建键失败: {}\\{}", node.hive, node.path))?;
    for v in &node.values {
        write_value(&key, v).with_context(|| format!("写值失败 {}\\{}\\{}", node.hive, node.path, v.name))?;
    }
    for sub in &node.subkeys {
        write_subtree(sub)?;
    }
    Ok(())
}

fn write_value(key: &Key, v: &RegValue) -> Result<()> {
    use windows_registry::Type as T;
    let ty = match v.kind {
        RegValueKind::Sz => T::String,
        RegValueKind::ExpandSz => T::ExpandString,
        RegValueKind::Dword => T::U32,
        RegValueKind::Qword => T::U64,
        RegValueKind::MultiSz => T::MultiString,
        RegValueKind::Binary => T::Bytes,
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&v.data)
        .context("base64 解码失败")?;
    key.set_bytes(v.name.as_str(), ty, &bytes)?;
    Ok(())
}

/// 递归删除整个子树
pub fn delete_subtree(hive: &str, path: &str) -> Result<()> {
    let root = hive_root(hive)?;
    // windows-registry 0.6 提供 remove_tree
    root.remove_tree(path).with_context(|| format!("删键失败: {hive}\\{path}"))?;
    Ok(())
}

/// 统计子树规模(键数+值数), 用于 dry-run 展示
pub fn count_subtree(node: &RegSubtree) -> (usize, usize) {
    let mut keys = 1;
    let mut vals = node.values.len();
    for c in &node.subkeys {
        let (k, v) = count_subtree(c);
        keys += k;
        vals += v;
    }
    (keys, vals)
}
