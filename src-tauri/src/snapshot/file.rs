//! 文件快照(原内容 base64) — 适用于小文件(.lnk 通常 < 4KB)
//! 大文件不入此模块, 直接交回收站方案处理。

use anyhow::Result;
use base64::Engine;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSnapshot {
    pub original_path: String,
    pub content_base64: String,
    pub size: u64,
}

const MAX_FILE_SIZE: u64 = 1024 * 1024 * 4; // 4MB 上限

pub fn snapshot(path: &str) -> Result<FileSnapshot> {
    let meta = std::fs::metadata(path)?;
    if meta.len() > MAX_FILE_SIZE {
        return Err(anyhow::anyhow!(
            "文件过大 ({} 字节), 拒绝快照 (上限 {})",
            meta.len(),
            MAX_FILE_SIZE
        ));
    }
    let bytes = std::fs::read(path)?;
    let content_base64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(FileSnapshot {
        original_path: path.to_string(),
        content_base64,
        size: meta.len(),
    })
}

pub fn restore(snap: &FileSnapshot) -> Result<()> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(&snap.content_base64)?;
    if let Some(parent) = std::path::Path::new(&snap.original_path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&snap.original_path, bytes)?;
    Ok(())
}
