//! sentry 进程内的子进程启动 helper, 复制自 GUI 的 sys_cmd

use std::process::Command;

pub fn cmd(program: &str) -> Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut c = Command::new(program);
    c.creation_flags(CREATE_NO_WINDOW);
    c
}
