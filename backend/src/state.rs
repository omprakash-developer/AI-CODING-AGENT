use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

use crate::{
    database::Database,
    graph::kuzu::KuzuGraph,
    models::{Task, TaskProposal},
};

#[derive(Clone)]
pub struct AppState {
    pub tasks: Arc<RwLock<HashMap<Uuid, Task>>>,
    pub events: Arc<RwLock<HashMap<Uuid, broadcast::Sender<String>>>>,
    pub approved_tasks: Arc<RwLock<HashSet<Uuid>>>,
    pub proposals: Arc<RwLock<HashMap<Uuid, TaskProposal>>>,
    pub database: Database,

    // Kuzu code graph
    pub kuzu: Arc<KuzuGraph>,
}

impl AppState {
    pub fn new(database: Database, kuzu: Arc<KuzuGraph>) -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            events: Arc::new(RwLock::new(HashMap::new())),
            approved_tasks: Arc::new(RwLock::new(HashSet::new())),
            proposals: Arc::new(RwLock::new(HashMap::new())),
            database,
            kuzu,
        }
    }
}
