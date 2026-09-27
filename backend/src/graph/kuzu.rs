use kuzu::{Connection, Database, SystemConfig};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct KuzuGraph {
    database: Arc<Database>,
    path: PathBuf,
}

impl KuzuGraph {
    // =====================================================
    // CREATE / OPEN KUZU DATABASE
    // =====================================================

    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();

        println!("========================================");
        println!("KUZU GRAPH INITIALIZATION");
        println!("Database path: {}", path.display());

        // Create parent directory if required
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create Kuzu directory: {}", e))?;
        }

        // Open Kuzu database
        let database = Database::new(&path, SystemConfig::default())
            .map_err(|e| format!("Failed to open Kuzu database: {}", e))?;

        let database = Arc::new(database);

        // Create a temporary connection.
        // The scope ensures the connection is dropped
        // before database is moved into KuzuGraph.
        {
            let connection = Connection::new(&database)
                .map_err(|e| format!("Failed to create Kuzu connection: {}", e))?;

            Self::initialize_schema(&connection)?;
        }

        println!("Kuzu database initialized successfully.");

        println!("========================================");

        Ok(Self { database, path })
    }

    // =====================================================
    // INITIALIZE GRAPH SCHEMA
    // =====================================================

    fn initialize_schema(connection: &Connection<'_>) -> Result<(), String> {
        // -------------------------------------------------
        // File node
        // -------------------------------------------------

        connection
            .query(
                r#"
                CREATE NODE TABLE IF NOT EXISTS File(
                    path STRING PRIMARY KEY,
                    language STRING
                )
                "#,
            )
            .map_err(|e| format!("Failed to create File table: {}", e))?;

        // -------------------------------------------------
        // Function node
        // -------------------------------------------------

        connection
            .query(
                r#"
                CREATE NODE TABLE IF NOT EXISTS Function(
                    id STRING PRIMARY KEY,
                    name STRING,
                    file STRING,
                    line INT64
                )
                "#,
            )
            .map_err(|e| format!("Failed to create Function table: {}", e))?;

        // -------------------------------------------------
        // File contains function
        // -------------------------------------------------

        connection
            .query(
                r#"
                CREATE REL TABLE IF NOT EXISTS CONTAINS(
                    FROM File TO Function
                )
                "#,
            )
            .map_err(|e| format!("Failed to create CONTAINS relation: {}", e))?;

        // -------------------------------------------------
        // Function calls function
        // -------------------------------------------------

        connection
            .query(
                r#"
                CREATE REL TABLE IF NOT EXISTS CALLS(
                    FROM Function TO Function
                )
                "#,
            )
            .map_err(|e| format!("Failed to create CALLS relation: {}", e))?;

        Ok(())
    }

    // =====================================================
    // GET DATABASE PATH
    // =====================================================

    pub fn path(&self) -> &Path {
        &self.path
    }

    // =====================================================
    // ADD FILE
    // =====================================================

    pub fn add_file(&self, path: &str, language: &str) -> Result<(), String> {
        let connection = Connection::new(&self.database)
            .map_err(|e| format!("Failed to create Kuzu connection: {}", e))?;

        let query = format!(
            "MERGE (f:File {{path: '{}', language: '{}'}})",
            escape_string(path),
            escape_string(language)
        );

        connection
            .query(&query)
            .map_err(|e| format!("Failed to add file '{}': {}", path, e))?;

        Ok(())
    }

    // =====================================================
    // ADD FUNCTION
    // =====================================================

    pub fn add_function(&self, id: &str, name: &str, file: &str, line: i64) -> Result<(), String> {
        let connection = Connection::new(&self.database)
            .map_err(|e| format!("Failed to create Kuzu connection: {}", e))?;

        let query = format!(
            r#"
            MERGE (f:Function {{
                id: '{}',
                name: '{}',
                file: '{}',
                line: {}
            }})
            "#,
            escape_string(id),
            escape_string(name),
            escape_string(file),
            line
        );

        connection
            .query(&query)
            .map_err(|e| format!("Failed to add function '{}': {}", name, e))?;

        Ok(())
    }

    // =====================================================
    // ADD CONTAINS RELATION
    // =====================================================

    pub fn add_contains_relation(&self, file: &str, function_id: &str) -> Result<(), String> {
        let connection = Connection::new(&self.database)
            .map_err(|e| format!("Failed to create Kuzu connection: {}", e))?;

        let query = format!(
            r#"
            MATCH (f:File {{path: '{}'}})
            MATCH (fn:Function {{id: '{}'}})
            MERGE (f)-[:CONTAINS]->(fn)
            "#,
            escape_string(file),
            escape_string(function_id)
        );

        connection
            .query(&query)
            .map_err(|e| format!("Failed to create CONTAINS relation: {}", e))?;

        Ok(())
    }

    // =====================================================
    // ADD CALLS RELATION
    // =====================================================

    pub fn add_calls_relation(&self, caller_id: &str, callee_id: &str) -> Result<(), String> {
        let connection = Connection::new(&self.database)
            .map_err(|e| format!("Failed to create Kuzu connection: {}", e))?;

        let query = format!(
            r#"
            MATCH (caller:Function {{id: '{}'}})
            MATCH (callee:Function {{id: '{}'}})
            MERGE (caller)-[:CALLS]->(callee)
            "#,
            escape_string(caller_id),
            escape_string(callee_id)
        );

        connection
            .query(&query)
            .map_err(|e| format!("Failed to create CALLS relation: {}", e))?;

        Ok(())
    }

    // =====================================================
    // TEST KUZU GRAPH
    // =====================================================

    pub fn test_graph(&self) -> Result<(), String> {
        println!("Testing Kuzu code graph...");

        // -------------------------------------------------
        // Create example file
        // -------------------------------------------------

        self.add_file("example.rs", "rust")?;

        // -------------------------------------------------
        // Create example functions
        // -------------------------------------------------

        self.add_function("example.rs::main", "main", "example.rs", 1)?;

        self.add_function("example.rs::hello", "hello", "example.rs", 5)?;

        // -------------------------------------------------
        // Connect file -> functions
        // -------------------------------------------------

        self.add_contains_relation("example.rs", "example.rs::main")?;

        self.add_contains_relation("example.rs", "example.rs::hello")?;

        // -------------------------------------------------
        // Create function call relationship
        // -------------------------------------------------

        self.add_calls_relation("example.rs::main", "example.rs::hello")?;

        // -------------------------------------------------
        // Query CALLS relationship
        // -------------------------------------------------

        let connection = Connection::new(&self.database)
            .map_err(|e| format!("Failed to create Kuzu connection: {}", e))?;

        let mut result = connection
            .query(
                r#"
                MATCH (caller:Function)-[:CALLS]->(callee:Function)
                RETURN caller.name, callee.name
                "#,
            )
            .map_err(|e| format!("Failed to query Kuzu graph: {}", e))?;

        println!("Kuzu CALLS graph:");

        // IMPORTANT:
        // Kuzu 0.11.3 returns Option from next(),
        // not Result<Option<_>, _>.
        while let Some(row) = result.next() {
            println!("{:?}", row);
        }

        println!("Kuzu graph test completed successfully.");

        Ok(())
    }
}

// =========================================================
// ESCAPE CYPHER STRING
// =========================================================

fn escape_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}
