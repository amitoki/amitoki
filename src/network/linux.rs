use super::PacketIo;
use amitoki_relay::{MAX_FRAME_SIZE, MIN_FRAME_SIZE};
use async_trait::async_trait;
use std::{
    ffi::CString,
    io, mem,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
};
use tokio::io::{unix::AsyncFd, Interest};

// LinuxのETH_P_ALL。バインド時はネットワークバイト順で渡す。
const ETHERNET_ALL_PROTOCOLS: u16 = 0x0003;

pub struct LinuxSocket {
    socket: AsyncFd<OwnedFd>,
    interface_index: i32,
}

impl LinuxSocket {
    pub fn set_receive_buffer(&self, bytes: usize) -> io::Result<()> {
        let requested = i32::try_from(bytes).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "受信バッファが大きすぎます"))?;
        let mut actual = 0_i32;
        let mut size = mem::size_of_val(&actual) as libc::socklen_t;
        // SAFETY: SO_RCVBUFに対応するi32の領域と、その実サイズを渡す。
        unsafe {
            check_status(libc::setsockopt(
                self.socket.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_RCVBUF,
                (&requested as *const i32).cast(),
                size,
            ))?;
            check_status(libc::getsockopt(
                self.socket.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_RCVBUF,
                (&mut actual as *mut i32).cast(),
                &mut size,
            ))?;
        }
        // Linuxは管理用領域を含め、要求量の2倍をSO_RCVBUFとして返す。
        if i64::from(actual) < i64::from(requested) * 2 {
            log::warn!("収集バッファはカーネル上限で制限されています: requested={requested} actual={actual}; net.core.rmem_maxを確認してください");
        }
        Ok(())
    }

    pub fn open(interface: &str, promiscuous: bool) -> io::Result<Self> {
        let name = CString::new(interface).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "interfaceにNULを含められません"))?;
        // SAFETY: nameはNUL終端し、この呼び出しの間生存する。
        let index = unsafe { libc::if_nametoindex(name.as_ptr()) };
        if index == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: 定数だけを渡す。成功したfdは直後にOwnedFdへ移して所有者を一つにする。
        let descriptor = unsafe {
            libc::socket(
                libc::AF_PACKET,
                libc::SOCK_RAW | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
                i32::from(ETHERNET_ALL_PROTOCOLS.to_be()),
            )
        };
        if descriptor < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: descriptorはこの関数で新しく作成した有効なfd。
        let socket = unsafe { OwnedFd::from_raw_fd(descriptor) };
        let address = packet_address(index as i32, ETHERNET_ALL_PROTOCOLS);
        // SAFETY: sockaddr_llをその実サイズとともに渡し、呼び出し中は生存する。
        check_status(unsafe {
            libc::bind(
                socket.as_raw_fd(),
                (&address as *const libc::sockaddr_ll).cast(),
                mem::size_of_val(&address) as libc::socklen_t,
            )
        })?;
        // 自分が注入したフレームを再び中継しない。Linux 4.20以降のオプション。
        set_option(&socket, libc::PACKET_IGNORE_OUTGOING, &1_i32)?;
        if promiscuous {
            enable_promiscuous(&socket, index as i32)?;
        }
        Ok(Self {
            socket: AsyncFd::new(socket)?,
            interface_index: index as i32,
        })
    }
}

fn packet_address(interface_index: i32, protocol: u16) -> libc::sockaddr_ll {
    libc::sockaddr_ll {
        sll_family: libc::AF_PACKET as u16,
        sll_protocol: protocol.to_be(),
        sll_ifindex: interface_index,
        sll_hatype: 0,
        sll_pkttype: 0,
        sll_halen: 6,
        sll_addr: [0; 8],
    }
}

fn set_option<T>(socket: &OwnedFd, option: i32, value: &T) -> io::Result<()> {
    // SAFETY: このモジュールが各optionに対応する型を渡す。ポインタとサイズはvalueに一致する。
    check_status(unsafe {
        libc::setsockopt(
            socket.as_raw_fd(),
            libc::SOL_PACKET,
            option,
            (value as *const T).cast(),
            mem::size_of_val(value) as libc::socklen_t,
        )
    })
}

fn enable_promiscuous(socket: &OwnedFd, interface_index: i32) -> io::Result<()> {
    let membership = libc::packet_mreq {
        mr_ifindex: interface_index,
        mr_type: libc::PACKET_MR_PROMISC as u16,
        mr_alen: 0,
        mr_address: [0; 8],
    };
    // ソケット単位のmembershipなので、終了時にインターフェースの状態を戻せる。
    set_option(socket, libc::PACKET_ADD_MEMBERSHIP, &membership)
}

fn check_status(status: i32) -> io::Result<()> {
    if status < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[async_trait]
impl PacketIo for LinuxSocket {
    async fn receive(&self, buffer: &mut [u8]) -> io::Result<usize> {
        self.socket
            .async_io(Interest::READABLE, |socket| {
                // SAFETY: bufferの書き込み可能な範囲だけ渡す。MSG_TRUNCは元の長さを返す。
                let length = unsafe { libc::recv(socket.as_raw_fd(), buffer.as_mut_ptr().cast(), buffer.len(), libc::MSG_TRUNC) };
                if length < 0 {
                    return Err(io::Error::last_os_error());
                }
                if length as usize > buffer.len() {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "受信フレームがバッファ上限を超えました"));
                }
                Ok(length as usize)
            })
            .await
    }

    async fn send(&self, frame: &[u8]) -> io::Result<()> {
        if !(MIN_FRAME_SIZE..=MAX_FRAME_SIZE).contains(&frame.len()) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "フレーム長が範囲外です"));
        }
        let protocol = u16::from_be_bytes([frame[12], frame[13]]);
        let mut address = packet_address(self.interface_index, protocol);
        address.sll_addr[..6].copy_from_slice(&frame[..6]);
        self.socket
            .async_io(Interest::WRITABLE, |socket| {
                // SAFETY: frame/addressは呼び出し中生存し、指定サイズと実サイズは一致する。
                let written = unsafe {
                    libc::sendto(
                        socket.as_raw_fd(),
                        frame.as_ptr().cast(),
                        frame.len(),
                        0,
                        (&address as *const libc::sockaddr_ll).cast(),
                        mem::size_of_val(&address) as libc::socklen_t,
                    )
                };
                if written < 0 {
                    return Err(io::Error::last_os_error());
                }
                if written as usize != frame.len() {
                    return Err(io::Error::new(io::ErrorKind::WriteZero, "フレーム全体を送信できませんでした"));
                }
                Ok(())
            })
            .await
    }
}
