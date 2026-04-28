use anyhow::Result;
use cap_std::fs::Dir;
use std::path::Path;
use std::sync::Arc;


pub struct ToolKit {
    workspace: String,
    restricted: bool,
    sandbox: Arc<Option<Dir>>,
}

impl Clone for ToolKit {
    fn clone(&self) -> Self {
        Self {
            workspace: self.workspace.clone(),
            restricted: self.restricted,
            sandbox: self.sandbox.clone(),
        }
    }
}

impl ToolKit {
    pub fn new(workspace: &Path, restricted: bool) -> Self {
        let sandbox = if restricted {
            Arc::new(Dir::open_ambient_dir(workspace).ok())
        } else {
            Arc::new(None)
        };

        Self {
            workspace: workspace.to_string_lossy().to_string(),
            restricted,
            sandbox,
        }
    }

    pub fn list_tools(&self) -> Vec<&str> {
        vec!["read_file", "write_file", "list_dir", "exec", "web_search"]
    }

    pub async fn execute(&self, tool: &str, input: &str) -> String {
        match tool {
            "read_file" => self.read_file(input).await,
            "write_file" => self.write_file(input).await,
            "list_dir" => self.list_dir(input).await,
            "exec" => self.exec(input).await,
            "web_search" => self.web_search(input).await,
            _ => format!("Unknown tool: {}", tool),
        }
    }

    async fn read_file(&self, path: &str) -> String {
        match self.sandbox.as_ref().as_ref() {
            Some(dir) => {
                match dir.read_to_string(Path::new(path)) {
                    Ok(content) => content,
                    Err(e) => format!("Error reading file: {}", e),
                }
            }
            None => {
                let full_path = std::path::Path::new(&self.workspace).join(path);
                match tokio::fs::read_to_string(&full_path).await {
                    Ok(c) => c,
                    Err(e) => format!("Error: {}", e),
                }
            }
        }
    }

    async fn write_file(&self, input: &str) -> String {
        let parts: Vec<&str> = input.splitn(2, ':').collect();
        if parts.len() != 2 {
            return "Invalid format. Use 'path:content'".to_string();
        }
        let path = parts[0];
        let content = parts[1];
        let full_path = std::path::Path::new(&self.workspace).join(path);
        match tokio::fs::write(&full_path, content).await {
            Ok(_) => format!("Wrote to {}", path),
            Err(e) => format!("Error writing file: {}", e),
        }
    }

    async fn list_dir(&self, path: &str) -> String {
        let full_path = std::path::Path::new(&self.workspace).join(path);
        match tokio::fs::read_dir(&full_path).await {
            Ok(mut entries) => {
                let mut result = vec![];
                while let Ok(Some(entry)) = entries.next_entry().await {
                    if let Ok(metadata) = entry.metadata().await {
                        let name = entry.file_name().to_string_lossy().to_string();
                        let typ = if metadata.is_dir() { "dir" } else { "file" };
                        result.push(format!("{} ({})", name, typ));
                    }
                }
                result.join("\n")
            }
            Err(e) => format!("Error listing directory: {}", e),
        }
    }

    async fn exec(&self, cmd: &str) -> String {
        if self.restricted {
            let dangerous = ["rm -rf", "dd if=", "format", "shutdown", "> /dev", "mkfs"];
            for d in &dangerous {
                if cmd.contains(d) {
                    return format!("Command blocked by security policy: {}", d);
                }
            }
        }
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if parts.is_empty() {
            return "Empty command".to_string();
        }
        let output = tokio::process::Command::new(parts[0])
            .args(&parts[1..])
            .current_dir(&self.workspace)
            .output()
            .await;
        match output {
            Ok(out) => {
                if out.status.success() {
                    String::from_utf8_lossy(&out.stdout).to_string()
                } else {
                    format!("Error: {}", String::from_utf8_lossy(&out.stderr))
                }
            }
            Err(e) => format!("Failed to execute: {}", e),
        }
    }

    async fn web_search(&self, query: &str) -> String {
        // Brave Search API integration (requires BRAVE_API_KEY env var)
        let api_key = match std::env::var("BRAVE_API_KEY") {
            Ok(k) => k,
            Err(_) => return "Web search unavailable: BRAVE_API_KEY not set".to_string(),
        };

        let client = reqwest::Client::new();

        match client
            .get("https://api.search.brave.com/res/v1/web/search")
            .query(&[("q", query), ("count", "5")])
            .header("Accept", "application/json")
            .header("X-Subscription-Token", &api_key)
            .send()
            .await
        {
            Ok(resp) => {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    let results = json["web"]["results"]
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .take(5)
                                .filter_map(|r| {
                                    let title = r["title"].as_str()?;
                                    let desc = r["description"].as_str().unwrap_or("");
                                    let url = r["url"].as_str().unwrap_or("");
                                    Some(format!("• **{}**\n  {}\n  {}", title, desc, url))
                                })
                                .collect::<Vec<_>>()
                                .join("\n\n")
                        })
                        .unwrap_or_else(|| "No results".to_string());
                    format!("Search results for '{}':\n\n{}", query, results)
                } else {
                    "Failed to parse search results".to_string()
                }
            }
            Err(e) => format!("Search request failed: {}", e),
        }
    }
}

pub mod persistence {
    use anyhow::Result;
    use rusqlite::Connection;

    pub struct Database {
        conn: Connection,
    }

    impl Database {
        pub fn new(path: &std::path::Path) -> Result<Self> {
            std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new(".")))?;
            let conn = Connection::open(path)?;

            conn.execute_batch("
                PRAGMA journal_mode=WAL;
                CREATE TABLE IF NOT EXISTS sessions (
                    id TEXT PRIMARY KEY,
                    channel TEXT,
                    user_id TEXT,
                    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                );
                CREATE TABLE IF NOT EXISTS memories (
                    id TEXT PRIMARY KEY,
                    session_id TEXT,
                    role TEXT,
                    content TEXT,
                    timestamp INTEGER,
                    FOREIGN KEY(session_id) REFERENCES sessions(id)
                );
                CREATE TABLE IF NOT EXISTS cot_traces (
                    id TEXT PRIMARY KEY,
                    session_id TEXT,
                    trace TEXT,
                    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                );
                CREATE TABLE IF NOT EXISTS usage_stats (
                    id TEXT PRIMARY KEY,
                    session_id TEXT,
                    model TEXT,
                    tokens INTEGER,
                    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                );
                CREATE INDEX IF NOT EXISTS idx_memories_session ON memories(session_id, timestamp);
            ")?;

            Ok(Self { conn })
        }

        pub fn ensure_session(&self, session_id: &str, channel: &str, user_id: &str) -> Result<()> {
            self.conn.execute(
                "INSERT OR IGNORE INTO sessions (id, channel, user_id) VALUES (?1, ?2, ?3)",
                rusqlite::params![session_id, channel, user_id],
            )?;
            Ok(())
        }

        pub fn save_message(&self, session_id: &str, role: &str, content: &str) -> Result<()> {
            let id = uuid::Uuid::new_v4().to_string();
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            self.conn.execute(
                "INSERT INTO memories (id, session_id, role, content, timestamp) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![id, session_id, role, content, ts],
            )?;
            Ok(())
        }

        pub fn get_history(&self, session_id: &str, limit: usize) -> Result<Vec<(String, String)>> {
            let mut stmt = self.conn.prepare(
                "SELECT role, content FROM memories WHERE session_id = ?1 ORDER BY timestamp ASC LIMIT ?2",
            )?;
            let rows = stmt.query_map(
                rusqlite::params![session_id, limit as i64],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )?;
            Ok(rows.filter_map(|r| r.ok()).collect())
        }

        pub fn track_usage(&self, session_id: &str, model: &str, tokens: i64) -> Result<()> {
            let id = uuid::Uuid::new_v4().to_string();
            self.conn.execute(
                "INSERT INTO usage_stats (id, session_id, model, tokens) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![id, session_id, model, tokens],
            )?;
            Ok(())
        }

        pub fn save_cot_trace(&self, session_id: &str, trace: &str) -> Result<()> {
            let id = uuid::Uuid::new_v4().to_string();
            self.conn.execute(
                "INSERT INTO cot_traces (id, session_id, trace) VALUES (?1, ?2, ?3)",
                rusqlite::params![id, session_id, trace],
            )?;
            Ok(())
        }

        pub fn get_total_usage(&self) -> Result<(i64, i64)> {
            let total_requests: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM memories WHERE role = 'user'",
                [],
                |row| row.get(0),
            ).unwrap_or(0);
            let total_tokens: i64 = self.conn.query_row(
                "SELECT COALESCE(SUM(tokens), 0) FROM usage_stats",
                [],
                |row| row.get(0),
            ).unwrap_or(0);
            Ok((total_requests, total_tokens))
        }
    }
}

pub mod migration {
    use anyhow::Result;
    use crate::config::Config;

    pub async fn migrate() -> Result<Config> {
        tracing::info!("Migrating from PicoClaw...");
        let config = Config::default();
        config.save()?;
        Ok(config)
    }
}
