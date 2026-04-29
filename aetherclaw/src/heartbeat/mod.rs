pub mod service {
    use std::path::PathBuf;
    use tokio::time::{interval, Duration};
    use tokio_util::sync::CancellationToken;
    use tracing::{info, warn};

    pub async fn run(minutes: u32, workspace: PathBuf, cancel: CancellationToken) {
        // Fire immediately on startup, then on schedule
        execute_heartbeat(&workspace).await;

        let mut ticker = interval(Duration::from_secs(minutes as u64 * 60));
        ticker.tick().await; // consume the immediate tick (already ran above)

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    execute_heartbeat(&workspace).await;
                }
                _ = cancel.cancelled() => {
                    info!("Heartbeat service shutting down...");
                    break;
                }
            }
        }
    }

    async fn execute_heartbeat(workspace: &PathBuf) {
        let heartbeat_file = workspace.join("HEARTBEAT.md");
        let content = match tokio::fs::read_to_string(&heartbeat_file).await {
            Ok(c) => c,
            Err(_) => {
                // Create a default HEARTBEAT.md if missing
                create_default_heartbeat(workspace).await;
                return;
            }
        };

        info!("Heartbeat: processing {} bytes from HEARTBEAT.md", content.len());
        let tasks = parse_heartbeat(&content);
        info!("Heartbeat: {} tasks found", tasks.len());

        for task in &tasks {
            if task.enabled {
                info!("Heartbeat task: [{}] {}", task.id, task.description);
                run_task(task, workspace).await;
            }
        }
    }

    #[derive(Debug)]
    struct HeartbeatTask {
        id: String,
        description: String,
        command: Option<String>,
        enabled: bool,
    }

    fn parse_heartbeat(content: &str) -> Vec<HeartbeatTask> {
        let mut tasks = vec![];
        let mut current_task: Option<HeartbeatTask> = None;

        for line in content.lines() {
            let trimmed = line.trim();

            // Task header: ## Task: <id>
            if let Some(rest) = trimmed.strip_prefix("## Task:").or(trimmed.strip_prefix("## task:")) {
                if let Some(task) = current_task.take() {
                    tasks.push(task);
                }
                current_task = Some(HeartbeatTask {
                    id: rest.trim().to_string(),
                    description: String::new(),
                    command: None,
                    enabled: true,
                });
                continue;
            }

            // Disabled marker
            if trimmed == "disabled" || trimmed == "- disabled" {
                if let Some(t) = current_task.as_mut() {
                    t.enabled = false;
                }
                continue;
            }

            // Description line
            if trimmed.starts_with("- desc:") || trimmed.starts_with("- description:") {
                let desc = trimmed.split_once(':').map(|(_, v)| v).unwrap_or("").trim().to_string();
                if let Some(t) = current_task.as_mut() {
                    t.description = desc;
                }
                continue;
            }

            // Shell command
            if trimmed.starts_with("- cmd:") || trimmed.starts_with("- command:") {
                let cmd = trimmed.split_once(':').map(|(_, v)| v).unwrap_or("").trim().to_string();
                if let Some(t) = current_task.as_mut() {
                    t.command = Some(cmd);
                }
            }
        }

        if let Some(task) = current_task {
            tasks.push(task);
        }

        tasks
    }

    async fn run_task(task: &HeartbeatTask, workspace: &PathBuf) {
        if let Some(cmd) = &task.command {
            let parts: Vec<&str> = cmd.split_whitespace().collect();
            if parts.is_empty() {
                return;
            }

            match tokio::process::Command::new(parts[0])
                .args(&parts[1..])
                .current_dir(workspace)
                .output()
                .await
            {
                Ok(out) => {
                    if out.status.success() {
                        info!(
                            "Heartbeat task [{}] succeeded: {}",
                            task.id,
                            String::from_utf8_lossy(&out.stdout).trim()
                        );
                    } else {
                        warn!(
                            "Heartbeat task [{}] failed: {}",
                            task.id,
                            String::from_utf8_lossy(&out.stderr).trim()
                        );
                    }
                }
                Err(e) => warn!("Heartbeat task [{}] error: {}", task.id, e),
            }
        }
    }

    async fn create_default_heartbeat(workspace: &PathBuf) {
        let default = r#"# AetherClaw Heartbeat Tasks
#
# Format:
#   ## Task: <unique-id>
#   - desc: Human-readable description
#   - cmd: shell command to run (runs in workspace directory)
#
# Add `disabled` below a task header to skip it.

## Task: health-check
- desc: Verify system is responsive
- cmd: echo "AetherClaw heartbeat OK"

## Task: workspace-cleanup
- desc: Remove temp files older than 7 days
- cmd: find . -name "*.tmp" -mtime +7 -delete
disabled
"#;
        let path = workspace.join("HEARTBEAT.md");
        if let Err(e) = tokio::fs::create_dir_all(workspace).await {
            warn!("Could not create workspace directory: {}", e);
            return;
        }
        let _ = tokio::fs::write(&path, default).await;
        info!("Created default HEARTBEAT.md at {}", path.display());
    }
}
