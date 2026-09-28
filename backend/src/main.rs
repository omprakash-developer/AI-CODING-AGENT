use std::env;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    http::Method,
    routing::{get, post},
    Router,
};

use tower_http::cors::{Any, CorsLayer};

use crate::{
    database::Database,
    graph::kuzu::KuzuGraph,
    routes::{
        graph::test_graph,
        memory::search_memory,
        sandbox::{run_revision_loop, run_sandbox, run_verification, test_sandbox},
        system::system_status,
        tasks::{approve_task, create_task, get_task, list_tasks, stream_task},
        tools::{git_branch, git_diff, git_status, list_files, read_file, write_file},
    },
    state::AppState,
};

// =========================================================
// MODULES
// =========================================================

mod agent;
mod database;
mod gemini;
mod graph;
mod models;
mod revision;
mod routes;
mod sandbox;
mod state;
mod tools;
mod verification;

// =========================================================
// MAIN
// =========================================================

#[tokio::main]
async fn main() {
    // -----------------------------------------------------
    // Load environment variables from .env
    // -----------------------------------------------------

    dotenvy::dotenv().ok();

    // -----------------------------------------------------
    // Initialize SQLite database
    // -----------------------------------------------------

    let database = Database::new("data/agent.db").expect("Failed to initialize SQLite database");

    println!(
        "SQLite database initialized at: {}",
        database.path().display()
    );

    // -----------------------------------------------------
    // Initialize Kuzu code graph
    // -----------------------------------------------------

    let kuzu = Arc::new(
        KuzuGraph::new("data/code_graph.kuzu").expect("Failed to initialize Kuzu code graph"),
    );

    println!("Kuzu code graph initialized successfully.");

    // -----------------------------------------------------
    // Create application state
    // -----------------------------------------------------

    let state = AppState::new(database, kuzu);

    // -----------------------------------------------------
    // Configure CORS
    // -----------------------------------------------------

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers(Any);

    // -----------------------------------------------------
    // Build API router
    // -----------------------------------------------------

    let app = Router::new()
        // =================================================
        // SYSTEM STATUS ROUTE
        // =================================================
        // Returns backend/model/transport info.
        //
        // GET /system/status
        .route("/system/status", get(system_status))
        // =================================================
        // TASK ROUTES
        // =================================================
        // Create a new coding task / List persistent task history
        .route("/tasks", post(create_task).get(list_tasks))
        // Get a task by ID
        .route("/tasks/{id}", get(get_task))
        // Stream task events using SSE
        .route("/tasks/{id}/stream", get(stream_task))
        // Approve a task proposal
        .route("/tasks/{id}/approve", post(approve_task))
        // =================================================
        // MEMORY ROUTES
        // =================================================
        // Search persistent agent memory
        //
        // Example:
        // GET /memory/search?q=login
        //
        // Optional:
        // GET /memory/search?q=login&limit=10
        .route("/memory/search", get(search_memory))
        // =================================================
        // KUZU CODE GRAPH ROUTES
        // =================================================
        // Test Kuzu code graph
        //
        // GET /graph/test
        .route("/graph/test", get(test_graph))
        // =================================================
        // DOCKER SANDBOX ROUTES
        // =================================================
        // Test Docker sandbox
        //
        // GET /sandbox/test
        .route("/sandbox/test", get(test_sandbox))
        // Run a command inside Docker sandbox
        //
        // POST /sandbox/run
        //
        // Example JSON:
        // {
        //     "command": "echo Hello"
        // }
        .route("/sandbox/run", post(run_sandbox))
        // =================================================
        // VERIFICATION ROUTES
        // =================================================
        // Run a verification command inside Docker
        //
        // POST /verification/run
        //
        // Example JSON:
        // {
        //     "command": "echo Verification Passed"
        // }
        .route("/verification/run", post(run_verification))
        // =================================================
        // REVISION LOOP ROUTES
        // =================================================
        // Run the agent revision loop
        //
        // POST /revision/run
        //
        // Example JSON:
        // {
        //     "command": "echo Revision Loop Passed",
        //     "max_attempts": 3
        // }
        .route("/revision/run", post(run_revision_loop))
        // =================================================
        // GIT TOOL ROUTES
        // =================================================
        // Git status
        .route("/tools/git/status", get(git_status))
        // Git diff
        .route("/tools/git/diff", get(git_diff))
        // Current Git branch
        .route("/tools/git/branch", get(git_branch))
        // =================================================
        // WORKSPACE FILE ROUTES
        // =================================================
        // List workspace files
        .route("/tools/files", get(list_files))
        // Read a workspace file
        .route("/tools/file", get(read_file))
        // Write a workspace file
        .route("/tools/file", post(write_file))
        // =================================================
        // APPLICATION STATE
        // =================================================
        .with_state(state)
        // =================================================
        // CORS
        // =================================================
        .layer(cors);

    // -----------------------------------------------------
    // Read HOST from environment
    // -----------------------------------------------------

    let host = env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string());

    // -----------------------------------------------------
    // Read PORT from environment
    // -----------------------------------------------------

    let port = env::var("PORT")
        .unwrap_or_else(|_| "3000".to_string())
        .parse::<u16>()
        .expect("PORT must be a valid number");

    // -----------------------------------------------------
    // Create server address
    // -----------------------------------------------------

    let address = SocketAddr::from((
        host.parse::<std::net::IpAddr>()
            .expect("HOST must be a valid IP address"),
        port,
    ));

    // -----------------------------------------------------
    // Startup information
    // -----------------------------------------------------

    println!("========================================");

    println!("       AI Coding Agent Backend");

    println!("========================================");

    println!("Server: http://{}", address);

    println!("SQLite: enabled");

    println!("sqlite-vec: enabled");

    println!("Kuzu Code Graph: enabled");

    println!("Kuzu Graph Test: /graph/test");

    println!("Docker Sandbox: enabled");

    println!("Docker Sandbox Test: /sandbox/test");

    println!("Docker Sandbox Run: /sandbox/run");

    println!("Verification: /verification/run");

    println!("Revision Loop: /revision/run");

    println!("SSE: enabled");

    println!("Gemini: enabled");

    println!("Git: enabled");

    println!("Memory Search: enabled");

    println!("========================================");

    // -----------------------------------------------------
    // Create TCP listener
    // -----------------------------------------------------

    let listener = tokio::net::TcpListener::bind(address)
        .await
        .expect("Failed to bind backend server");

    // -----------------------------------------------------
    // Start Axum server
    // -----------------------------------------------------

    axum::serve(listener, app)
        .await
        .expect("Backend server failed");
}
