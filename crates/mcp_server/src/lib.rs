pub mod state;
pub mod tools;

pub use state::{EditorSharedState, EditorSyncEvent};
pub use tools::TexWordMcpServer;

use rmcp::{
    serve_server,
    transport::{
        io::stdio,
        streamable_http_server::{
            StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
        },
    },
};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

/// Start the embedded MCP Streamable HTTP server (e.g. `http://127.0.0.1:3847/mcp`)
/// alongside the GPUI desktop window so external AI edits update the live UI.
pub async fn serve_http(
    state: Arc<RwLock<EditorSharedState>>,
    addr: SocketAddr,
    ct: CancellationToken,
) -> anyhow::Result<()> {
    let config = StreamableHttpServerConfig::default().with_cancellation_token(ct.clone());
    let service: StreamableHttpService<TexWordMcpServer, LocalSessionManager> =
        StreamableHttpService::new(
            move || Ok(TexWordMcpServer::new(state.clone())),
            Default::default(),
            config,
        );

    let router = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("MCP Streamable HTTP listening at http://{}/mcp", addr);

    axum::serve(listener, router)
        .with_graceful_shutdown(async move { ct.cancelled().await })
        .await?;
    Ok(())
}

/// Start a pure stdio MCP server (for clients like Claude Desktop / Cursor launching the binary directly).
pub async fn serve_stdio(state: Arc<RwLock<EditorSharedState>>) -> anyhow::Result<()> {
    let server = TexWordMcpServer::new(state);
    let service = serve_server(server, stdio()).await?;
    service.waiting().await?;
    Ok(())
}
