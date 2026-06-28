//! 每 PID 上行字节数采样
//!
//! 当前简化实现:
//!  - GetExtendedTcpTable (IPv4) 拿所有 TCP 连接的 (pid, 4元组)
//!  - SetPerTcpConnectionEStats 首次启用每个连接的 EStats 计数器
//!  - GetPerTcpConnectionEStats 读 DataBytesOut
//!  - 累加同 PID 的所有连接 → (pid, total_bytes_out)
//!
//! Phase 2 加 IPv6 + UDP/QUIC ETW

use anyhow::{anyhow, Result};
use std::collections::HashMap;

#[allow(non_snake_case)]
mod ffi {
    use std::ffi::c_void;

    pub const ERROR_SUCCESS: u32 = 0;
    pub const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
    pub const AF_INET: u32 = 2;
    pub const TCP_TABLE_OWNER_PID_ALL: u32 = 5;

    // TcpConnectionEstatsData = 1
    pub const TCP_ESTATS_DATA_ROD_V0: u32 = 1;

    #[repr(C)]
    pub struct MIB_TCPROW_OWNER_PID {
        pub dwState: u32,
        pub dwLocalAddr: u32,
        pub dwLocalPort: u32,
        pub dwRemoteAddr: u32,
        pub dwRemotePort: u32,
        pub dwOwningPid: u32,
    }

    #[repr(C)]
    pub struct MIB_TCPTABLE_OWNER_PID {
        pub dwNumEntries: u32,
        pub table: [MIB_TCPROW_OWNER_PID; 1],
    }

    // TCP_ESTATS_DATA_ROD_v0
    #[repr(C)]
    #[derive(Default, Debug)]
    pub struct TCP_ESTATS_DATA_ROD_V0_S {
        pub DataBytesOut: u64,
        pub DataSegsOut: u64,
        pub DataBytesIn: u64,
        pub DataSegsIn: u64,
        pub SegsOut: u64,
        pub SegsIn: u64,
        pub SoftErrors: u32,
        pub SoftErrorReason: u32,
        pub SndUna: u32,
        pub SndNxt: u32,
        pub SndMax: u32,
        pub ThruBytesAcked: u64,
        pub RcvNxt: u32,
        pub ThruBytesReceived: u64,
    }

    // TCP_ESTATS_DATA_RW_v0
    #[repr(C)]
    pub struct TCP_ESTATS_DATA_RW_V0 {
        pub EnableCollection: u8,
    }

    #[link(name = "iphlpapi")]
    unsafe extern "system" {
        pub fn GetExtendedTcpTable(
            pTcpTable: *mut c_void,
            pdwSize: *mut u32,
            bOrder: i32,
            ulAf: u32,
            TableClass: u32,
            Reserved: u32,
        ) -> u32;

        pub fn SetPerTcpConnectionEStats(
            Row: *const MIB_TCPROW_OWNER_PID,
            EstatsType: u32,
            Rw: *const c_void,
            RwVersion: u32,
            RwSize: u32,
            Offset: u32,
        ) -> u32;

        pub fn GetPerTcpConnectionEStats(
            Row: *const MIB_TCPROW_OWNER_PID,
            EstatsType: u32,
            Rw: *mut c_void,
            RwVersion: u32,
            RwSize: u32,
            Ros: *mut c_void,
            RosVersion: u32,
            RosSize: u32,
            Rod: *mut c_void,
            RodVersion: u32,
            RodSize: u32,
        ) -> u32;
    }
}

/// 判断 dwRemoteAddr (network byte order, IPv4) 是否为公网地址。
/// 跳过 loopback / 私网 / link-local / multicast / 0.0.0.0 / 监听态。
///
/// 关键 bug 修复: clash 这类代理软件本地监听 127.0.0.1:7890, 浏览器/app 通过
/// 它访问外网. clash 把下载内容通过这个 loopback TCP 转发出去 — 在 TCP_ESTATS
/// 视角里 clash 的 DataBytesOut 暴涨。但这其实是用户下载量, 不是"PCDN 上传"。
/// 过滤掉非公网连接后, 真正流入 PCDN 监控的就只剩"对外网真实 upload"。
fn is_internet_address(addr_net: u32) -> bool {
    // Windows little-endian: u32 字节序 [b1,b2,b3,b4], 对应 IPv4 "b1.b2.b3.b4"
    let b1 = (addr_net & 0xFF) as u8;
    let b2 = ((addr_net >> 8) & 0xFF) as u8;
    if addr_net == 0 { return false; }            // 0.0.0.0 listening
    if b1 == 127 { return false; }                // loopback
    if b1 == 10 { return false; }                 // 10.0.0.0/8
    if b1 == 172 && (16..=31).contains(&b2) { return false; }  // 172.16/12
    if b1 == 192 && b2 == 168 { return false; }   // 192.168/16
    if b1 == 169 && b2 == 254 { return false; }   // link-local
    if b1 >= 224 { return false; }                // multicast / reserved
    true
}

/// 拿当前所有 TCP 连接的 (pid, 上行总字节)。返回按 pid 聚合。
/// 仅累加远端为公网地址的连接, 滤掉 loopback / 私网 (clash 代理转发会被错算上传)。
pub fn sample_per_pid_bytes_out() -> Result<HashMap<u32, u64>> {
    let rows = read_tcp_table_v4()?;
    let mut acc: HashMap<u32, u64> = HashMap::new();

    for row in rows {
        // 过滤非公网连接
        if !is_internet_address(row.dwRemoteAddr) {
            continue;
        }

        // 首次见的连接启用 EStats data 采集
        let rw = ffi::TCP_ESTATS_DATA_RW_V0 { EnableCollection: 1 };
        unsafe {
            ffi::SetPerTcpConnectionEStats(
                &row,
                ffi::TCP_ESTATS_DATA_ROD_V0,
                &rw as *const _ as *const _,
                0,
                std::mem::size_of::<ffi::TCP_ESTATS_DATA_RW_V0>() as u32,
                0,
            );
        }

        // 读 ROD
        let mut rod = ffi::TCP_ESTATS_DATA_ROD_V0_S::default();
        let ret = unsafe {
            ffi::GetPerTcpConnectionEStats(
                &row,
                ffi::TCP_ESTATS_DATA_ROD_V0,
                std::ptr::null_mut(),
                0,
                0,
                std::ptr::null_mut(),
                0,
                0,
                &mut rod as *mut _ as *mut _,
                0,
                std::mem::size_of::<ffi::TCP_ESTATS_DATA_ROD_V0_S>() as u32,
            )
        };
        if ret == ffi::ERROR_SUCCESS {
            *acc.entry(row.dwOwningPid).or_insert(0) += rod.DataBytesOut;
        }
    }

    Ok(acc)
}

fn read_tcp_table_v4() -> Result<Vec<ffi::MIB_TCPROW_OWNER_PID>> {
    let mut size: u32 = 0;
    unsafe {
        ffi::GetExtendedTcpTable(
            std::ptr::null_mut(),
            &mut size,
            0,
            ffi::AF_INET,
            ffi::TCP_TABLE_OWNER_PID_ALL,
            0,
        );
    }
    if size == 0 {
        return Ok(Vec::new());
    }

    let mut buf: Vec<u8> = vec![0; size as usize];
    let ret = unsafe {
        ffi::GetExtendedTcpTable(
            buf.as_mut_ptr() as *mut _,
            &mut size,
            0,
            ffi::AF_INET,
            ffi::TCP_TABLE_OWNER_PID_ALL,
            0,
        )
    };
    if ret != ffi::ERROR_SUCCESS {
        return Err(anyhow!("GetExtendedTcpTable 返回 {ret}"));
    }

    let table = buf.as_ptr() as *const ffi::MIB_TCPTABLE_OWNER_PID;
    let count = unsafe { (*table).dwNumEntries } as usize;
    if count == 0 {
        return Ok(Vec::new());
    }
    let rows_ptr = unsafe { (*table).table.as_ptr() };
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        unsafe {
            let r = &*rows_ptr.add(i);
            out.push(ffi::MIB_TCPROW_OWNER_PID {
                dwState: r.dwState,
                dwLocalAddr: r.dwLocalAddr,
                dwLocalPort: r.dwLocalPort,
                dwRemoteAddr: r.dwRemoteAddr,
                dwRemotePort: r.dwRemotePort,
                dwOwningPid: r.dwOwningPid,
            });
        }
    }
    Ok(out)
}
