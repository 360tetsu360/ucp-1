use std::net::SocketAddr;

use ucp::{UcpListener, UcpSession};

#[tokio::main]
async fn main() {
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    dbg!(time);
    let addr: SocketAddr = "127.0.0.1:19132".parse().unwrap();
    let mut ucp = UcpListener::bind(addr, 0x114514, "MCPE;Dedicated Server;390;1.14.60;0;10;13253860892328930865;Bedrock level;Survival;1;19132;19133;".to_owned())
        .await
        .unwrap();
    loop {
        let incoming = ucp.accept().await.unwrap();
        tokio::spawn(handle(incoming));
    }
}

async fn handle(session: UcpSession) {
    loop {
        let a = session.recv().await.unwrap();
    }
}
