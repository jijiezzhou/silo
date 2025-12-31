use mcp_server::database::Database;
use mcp_server::state::AppState;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    init_tracing();

    // "Zero-panic" entrypoint: any error becomes a JSON-RPC error response from the server loop.
    let db = match Database::new("./data").await {
        Ok(db) => Arc::new(db),
        Err(e) => {
            eprintln!("Failed to initialize database: {e}");
            // Still start the server (tools like list_files/read_file should work).
            Arc::new(Database::disabled(e.to_string()))
        }
    };

    let state = match AppState::new(db).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to initialize app state: {e}");
            return;
        }
    };

    if let Err(e) = mcp_server::server::run_stdio_server(state).await {
        eprintln!("Server stopped with error: {e}");
    }
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    // Logs go to stderr by default. Keep stdout clean for JSON-RPC.
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .without_time()
        .init();
}


