//! NICに必要なcapabilityを外部実行ファイルへ渡さない。OSサンドボックスではない。
use std::io;
use tokio::process::Command;

pub(crate) fn harden(command: &mut Command) {
    // fork後はメモリ確保やロックを使わず、システムコールだけで権限を落とす。
    unsafe {
        command.pre_exec(drop_capabilities);
    }
}

fn drop_capabilities() -> io::Result<()> {
    #[repr(C)]
    struct Header {
        version: u32,
        pid: i32,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Capabilities {
        effective: u32,
        permitted: u32,
        inheritable: u32,
    }
    // Linux capability ABI v3は64ビットの集合を32ビットの2要素で表す。
    const CAPABILITY_ABI_V3: u32 = 0x20080522;
    let header = Header {
        version: CAPABILITY_ABI_V3,
        pid: 0,
    };
    let empty = [Capabilities {
        effective: 0,
        permitted: 0,
        inheritable: 0,
    }; 2];
    let status = unsafe {
        if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
            return Err(io::Error::last_os_error());
        }
        if libc::prctl(libc::PR_CAP_AMBIENT, libc::PR_CAP_AMBIENT_CLEAR_ALL, 0, 0, 0) != 0 {
            return Err(io::Error::last_os_error());
        }
        libc::syscall(libc::SYS_capset, &header, empty.as_ptr())
    };
    if status != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
