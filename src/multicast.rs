use socket2::{Domain, Protocol, SockAddr, Socket, Type};
use std::io;
use std::net::{Ipv4Addr, SocketAddrV4};

use crate::order::Order;

pub struct MulticastSender {
    socket: Socket,
    dest: SockAddr,
}

impl MulticastSender {
    pub fn new(group: Ipv4Addr, port: u16) -> io::Result<Self> {
        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;

        // TTL = 1: local subnet only
        socket.set_multicast_ttl_v4(1)?;

        let dest = SockAddr::from(SocketAddrV4::new(group, port));

        eprintln!("Multicast sender ready on {}:{}", group, port);

        Ok(Self { socket, dest })
    }

    pub fn send_order(&self, order: &Order) -> io::Result<()> {
        let msg = order.to_wire();
        self.socket.send_to(msg.as_bytes(), &self.dest)?;
        Ok(())
    }

    pub fn send_cancel(&self, order_id: u64, current_time: f64) -> io::Result<()> {
        let msg = crate::order::cancel_to_wire(order_id, current_time);
        self.socket.send_to(msg.as_bytes(), &self.dest)?;
        Ok(())
    }
}
