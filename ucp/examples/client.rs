use std::net::SocketAddr;

use ucp::{Reliability, UcpSession};

#[tokio::main]
async fn main() {
    let local: SocketAddr = "127.0.0.1:19130".parse().unwrap();
    let remote: SocketAddr = "127.0.0.1:19132".parse().unwrap();
    tokio::select! {
        re = UcpSession::connect(local, remote, 0x114514) => {
            handle(re.unwrap()).await;
        },
        _ = tokio::time::sleep(std::time::Duration::from_secs(10)) => {}
    };
}

async fn handle(mut session: UcpSession) {
    loop {
        session
            .send(&[0xfe; 0x8000], Reliability::ReliableOrdered)
            .await
            .unwrap();
        let packet = session.recv().await.unwrap();
        if packet[0] == 0xfe {
            println!("Game packet!");
        }
    }
}
