//! 文件动作: delete (可还原)

use anyhow::Result;

pub fn delete_file(path: &str) -> Result<()> {
    std::fs::remove_file(path)?;
    Ok(())
}
