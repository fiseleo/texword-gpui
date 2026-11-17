mod editor;
mod math_view;
mod theme;

use editor::TexWordEditor;
use gpui::{App, AppContext as _, Bounds, QuitMode, WindowBounds, WindowOptions, px, size};
use gpui_component::Root;
use mcp_server::{EditorSharedState, serve_http, serve_stdio};
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

fn detect_default_manuscript() -> Option<PathBuf> {
    let args: Vec<String> = std::env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--project")
        && let Some(path) = args.get(pos + 1)
    {
        return Some(PathBuf::from(path));
    }
    let default_book = PathBuf::from(r"z:\書\書稿");
    if default_book.exists() {
        return Some(default_book);
    }
    None
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "texword_app=info,mcp_server=info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let project_path = detect_default_manuscript();
    let shared_state = EditorSharedState::new_shared(project_path);

    // Pure headless stdio MCP mode (`texword_app --mcp-stdio --project z:\書\書稿`)
    if std::env::args().any(|a| a == "--mcp-stdio") {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        rt.block_on(serve_stdio(shared_state))?;
        return Ok(());
    }

    let mcp_addr: SocketAddr = "127.0.0.1:3847".parse()?;
    let mcp_endpoint = format!("http://{}/mcp", mcp_addr);
    let ct = CancellationToken::new();

    // Start background Tokio MCP Streamable HTTP server
    {
        let state_clone = shared_state.clone();
        let ct_clone = ct.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("failed to start tokio runtime for MCP");
            if let Err(e) = rt.block_on(serve_http(state_clone, mcp_addr, ct_clone)) {
                tracing::error!("MCP HTTP server error: {e}");
            }
        });
    }

    let ct_on_quit = ct.clone();
    gpui_platform::application()
        .with_quit_mode(QuitMode::LastWindowClosed)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);

            let bounds = Bounds::centered(None, size(px(1420.0), px(900.0)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            };

            let shared_for_win = shared_state.clone();
            let endpoint_for_win = mcp_endpoint.clone();
            cx.open_window(options, move |window, cx| {
                window.set_window_title("TeXWord GPUI — 類 Word 即時 LaTeX 編輯器 (含 MCP)");
                let editor_view = cx.new(|cx| {
                    TexWordEditor::new(shared_for_win, endpoint_for_win, window, cx)
                });
                cx.new(|cx| Root::new(editor_view, window, cx))
            })
            .expect("failed to open GPUI window");

            cx.on_app_quit(move |_| {
                ct_on_quit.cancel();
                async {}
            })
            .detach();

            cx.activate(true);
        });

    Ok(())
}
