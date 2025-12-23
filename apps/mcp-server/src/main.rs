mod database;
mod server;
mod tools;

use crate::database::Database;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    // "Zero-panic" entrypoint: any error becomes a JSON-RPC error response from the server loop.
    let db = match Database::new("./data").await {
        Ok(db) => Arc::new(db),
        Err(e) => {
            eprintln!("Failed to initialize database: {e}");
            // Still start the server (tools like list_files/read_file should work).
            Arc::new(Database::disabled(e.to_string()))
        }
    };

    if let Err(e) = server::run_stdio_server(db).await {
        eprintln!("Server stopped with error: {e}");
    }
}


