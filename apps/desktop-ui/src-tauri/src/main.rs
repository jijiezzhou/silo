#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::State;
use tokio::sync::Mutex;

struct AppCtx {
    app: Mutex<Option<mcp_server::api::SiloApp>>,
}

impl AppCtx {
    async fn get_or_init(&self) -> Result<mcp_server::api::SiloApp, String> {
        let mut guard = self.app.lock().await;
        if let Some(app) = guard.as_ref() {
            // Clone by reusing the shared state (cheap)
            return Ok(mcp_server::api::SiloApp {
                state: app.state.clone(),
            });
        }
        let app = mcp_server::api::SiloApp::new().await?;
        let clone = mcp_server::api::SiloApp {
            state: app.state.clone(),
        };
        *guard = Some(app);
        Ok(clone)
    }
}

#[tauri::command]
async fn get_config(state: State<'_, AppCtx>) -> Result<serde_json::Value, String> {
    let app = state.get_or_init().await?;
    Ok(app.get_config().await)
}

#[tauri::command]
async fn index_home(
    state: State<'_, AppCtx>,
    max_files: Option<u64>,
    concurrency: Option<usize>,
) -> Result<mcp_server::indexer::IndexSummary, String> {
    let app = state.get_or_init().await?;
    app.index_home(max_files, concurrency).await
}

#[tauri::command]
async fn search(
    state: State<'_, AppCtx>,
    query: String,
    top_k: Option<usize>,
) -> Result<serde_json::Value, String> {
    let app = state.get_or_init().await?;
    app.search(query, top_k.unwrap_or(5)).await
}

fn main() {
    tauri::Builder::default()
        .manage(AppCtx {
            app: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![get_config, index_home, search])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}


