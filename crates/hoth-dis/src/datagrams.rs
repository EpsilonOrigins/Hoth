use super::PDU_BYTES;
use std::{
    io,
    net::{SocketAddr, UdpSocket},
};

pub const PER_CALL: usize = 64;
pub type Packet = [u8; PDU_BYTES];

#[cfg(target_os = "linux")]
pub fn send(
    socket: &UdpSocket,
    destination: SocketAddr,
    packets: &[Packet],
) -> Result<(), (usize, io::Error)> {
    use std::os::fd::AsRawFd;

    let address = socket2::SockAddr::from(destination);
    let mut vectors: [libc::iovec; PER_CALL] = std::array::from_fn(|index| {
        // sendmmsg accepts mutable C pointers because these types are also used
        // by recvmsg. The send operation reads the packet and address buffers.
        let data = packets.get(index).map_or(std::ptr::null_mut(), |packet| {
            packet.as_ptr().cast_mut().cast()
        });
        libc::iovec {
            iov_base: data,
            iov_len: PDU_BYTES,
        }
    });
    // SAFETY: all fields are integer values or pointers with valid zero values.
    let mut messages: [libc::mmsghdr; PER_CALL] = unsafe { std::mem::zeroed() };
    for (vector, message) in vectors.iter_mut().zip(&mut messages[..packets.len()]) {
        message.msg_hdr.msg_iov = vector;
        message.msg_hdr.msg_iovlen = 1;
        message.msg_hdr.msg_name = address.as_ptr().cast_mut().cast();
        message.msg_hdr.msg_namelen = address.len();
    }

    let mut sent = 0;
    while sent < packets.len() {
        // SAFETY: each header points to one live vector with a fully
        // initialized packet and a live address. The socket owns its fd and
        // the kernel only writes per-message lengths into the headers.
        let count = unsafe {
            libc::sendmmsg(
                socket.as_raw_fd(),
                messages[sent..].as_mut_ptr(),
                (packets.len() - sent) as libc::c_uint,
                libc::MSG_DONTWAIT,
            )
        };
        if count < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err((sent, error));
        }
        if count == 0 {
            return Err((sent, io::Error::other("sendmmsg made no progress")));
        }
        sent += count as usize;
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn send(
    socket: &UdpSocket,
    destination: SocketAddr,
    packets: &[Packet],
) -> Result<(), (usize, io::Error)> {
    for (index, packet) in packets.iter().enumerate() {
        match socket.send_to(packet, destination) {
            Ok(PDU_BYTES) => {}
            Ok(_) => return Err((index, io::Error::other("incomplete UDP datagram"))),
            Err(error) => return Err((index, error)),
        }
    }
    Ok(())
}
