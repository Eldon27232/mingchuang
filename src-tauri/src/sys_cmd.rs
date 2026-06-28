//! Windows 下起子进程时统一加 CREATE_NO_WINDOW, 避免 GUI 父进程下闪黑框

use std::process::Command;

#[cfg(windows)]
pub fn cmd(program: &str) -> Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut c = Command::new(program);
    c.creation_flags(CREATE_NO_WINDOW);
    c
}

#[cfg(not(windows))]
pub fn cmd(program: &str) -> Command {
    Command::new(program)
}
