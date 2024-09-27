pub(crate) mod packets;

use packets::USRPPacket;
use std::io::Error;
use std::net::SocketAddr;
use std::sync::atomic::AtomicU32;
use tokio::net::UdpSocket;
pub struct USRPClient {
    rx: SocketAddr,
    tx: SocketAddr,
    pub local_addr: SocketAddr,

    rx_socket: Option<UdpSocket>,
    tx_socket: Option<UdpSocket>,

    sequence_number: AtomicU32,
}

impl USRPClient {
    /// Create a new USRPClient
    ///
    /// tx: The address to send packets to
    /// rx: The address to receive packets from
    pub fn new(rx: SocketAddr, tx: SocketAddr) -> Self {
        let local_addr: SocketAddr = if tx.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" }
            .parse()
            .unwrap();
        Self {
            rx,
            tx,
            local_addr,

            rx_socket: None,
            tx_socket: None,

            sequence_number: AtomicU32::new(0),
        }
    }

    pub async fn connect(&mut self) -> Result<(), Error> {
        let tx_socket = UdpSocket::bind(&self.local_addr).await?;
        tx_socket.connect(&self.tx).await?;

        let rx_socket = UdpSocket::bind(&self.rx).await?;
        self.tx_socket = Some(tx_socket);
        self.rx_socket = Some(rx_socket);

        Ok(())
    }

    pub async fn recv(&self) -> Option<USRPPacket> {
        if let Some(rx_socket) = &self.rx_socket {
            let mut buffer = [0; 1024];
            let recv = rx_socket.recv_from(&mut buffer).await;
            if let Ok((size, _)) = recv {
                let packet = USRPPacket::from_bytes(&buffer[..size]);
                return Some(packet);
            }
        }
        None
    }

    pub fn get_and_increment_sequence_number(&self) -> u32 {
        self.sequence_number
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    }

    pub async fn send(&self, packet: USRPPacket) -> Result<usize, Error> {
        if let Some(tx_socket) = &self.tx_socket {
            let bytes = packet.to_bytes();
            return tx_socket.send(&bytes).await;
        }
        Ok(0)
    }
}
