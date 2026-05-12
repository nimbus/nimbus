use std::net::SocketAddr;

use axum::{Router, serve};
use reqwest::Client;
use tokio::task::JoinHandle;

pub struct ServerFixture {
    addr: SocketAddr,
    client: Client,
    server: JoinHandle<()>,
}

impl ServerFixture {
    pub async fn start(app: Router) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener should bind");
        let addr = listener.local_addr().expect("listener should have addr");
        let server = tokio::spawn(async move {
            serve(listener, app).await.expect("server should run");
        });
        Self {
            addr,
            client: Client::new(),
            server,
        }
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    pub fn http_url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }

    pub fn ws_url(&self, path: &str) -> String {
        format!("ws://{}{}", self.addr, path)
    }
}

impl Drop for ServerFixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}
