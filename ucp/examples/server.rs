use std::net::SocketAddr;

use ucp::{UcpListener, UcpSession};

#[tokio::main]
async fn main() {
    let addr: SocketAddr = "127.0.0.1:19132".parse().unwrap();
    let mut ucp = UcpListener::bind(addr, 0x114514, "MCPE;Dedicated Server;390;1.14.60;0;10;13253860892328930865;Bedrock level;Survival;1;19132;19133;".to_owned())
        .await
        .unwrap();
    loop {
        let into_session = ucp.accept().await.unwrap();
        tokio::spawn(async move {
            tokio::select! {
                re = into_session => {
                    if let Ok(session) = re {
                        handle(session).await
                    }
                },
                _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {}
            };
        });
    }
}

async fn handle(mut session: UcpSession) {
    loop {
        let packet = session.recv().await.unwrap();
        if packet[0] == 0xfe {
            println!("Game packet!");
        }
    }
}
