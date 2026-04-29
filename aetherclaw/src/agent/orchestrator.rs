use super::cot::{Action, AgentType, ChainOfThought};
use super::specialized;
use crate::bus::{InboundMessage, OutboundMessage};
use crate::llm::ModelRouter;
use crate::tools::ToolKit;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Clone)]
pub struct MultiAgentOrchestrator {
    llm: ModelRouter,
    tools: ToolKit,
    bus_tx: mpsc::Sender<OutboundMessage>,
    workspace: PathBuf,
    cot_trace_dir: PathBuf,
    db: Arc<tokio::sync::Mutex<crate::tools::persistence::Database>>,
}

impl MultiAgentOrchestrator {
    pub fn new(
        llm: ModelRouter,
        tools: ToolKit,
        bus_tx: mpsc::Sender<OutboundMessage>,
        workspace: &std::path::Path,
        db: Arc<tokio::sync::Mutex<crate::tools::persistence::Database>>,
    ) -> Self {
        Self {
            llm,
            tools,
            bus_tx,
            workspace: workspace.to_path_buf(),
            cot_trace_dir: workspace.join("cot_traces"),
            db,
        }
    }

    pub async fn handle_request(&mut self, msg: InboundMessage) {
        let session_id = format!("{}-{}", msg.session_key, Uuid::new_v4());
        info!("Handling request: session={}", &session_id[..16.min(session_id.len())]);

        // Persist user message
        {
            let db = self.db.lock().await;
            let _ = db.ensure_session(&msg.session_key, &msg.channel, &msg.sender);
            let _ = db.save_message(&msg.session_key, "user", &msg.content);
        }

        let mut cot = ChainOfThought::new(
            15,
            format!(
                "User: {} | Channel: {} | Session: {}",
                msg.content, msg.channel, session_id
            ),
        );

        let is_build = msg.content.to_lowercase().contains("build")
            || msg.content.to_lowercase().contains("deploy")
            || msg.content.to_lowercase().contains("release");

        let response = if is_build {
            self.handle_build_pipeline(&msg, &mut cot, &session_id).await
        } else {
            self.handle_standard_query(&msg, &mut cot, &session_id).await
        };

        // Persist assistant response
        {
            let db = self.db.lock().await;
            let _ = db.save_message(&msg.session_key, "assistant", &response);
        }

        self.send_reply(&msg.session_key, &response, msg.channel.clone()).await;
        self.save_cot_trace(&session_id, &cot.export_trace()).await;
    }

    async fn handle_build_pipeline(
        &self,
        msg: &InboundMessage,
        cot: &mut ChainOfThought,
        _session_id: &str,
    ) -> String {
        cot.add_thought(
            "Analyzing build request".to_string(),
            Action::Think { reasoning: "Determine build targets and pipeline steps".to_string() },
            0.9,
        );

        // Step 1: Build
        let builder = specialized::BuilderAgent::new(self.tools.clone(), self.workspace.clone());
        cot.add_thought(
            "Dispatching Builder Agent".to_string(),
            Action::SpawnAgent { agent_type: AgentType::Builder, task: msg.content.clone() },
            0.95,
        );

        let build_result = builder.execute(&msg.content).await;
        match build_result {
            Err(e) => {
                cot.add_observation(format!("Build failed: {}", e));
                format!("Build failed: {}", e)
            }
            Ok(artifact) => {
                cot.add_observation(format!("Build successful: {}", artifact));

                // Step 2: Security audit
                let security = specialized::SecurityAgent::new(self.tools.clone());
                cot.add_thought(
                    "Dispatching Security Agent".to_string(),
                    Action::SpawnAgent {
                        agent_type: AgentType::Security,
                        task: format!("Audit {}", artifact),
                    },
                    0.92,
                );

                match security.execute(&artifact).await {
                    Err(e) => {
                        warn!("Security audit failed: {}", e);
                        cot.add_observation(format!("Security audit error: {}", e));
                    }
                    Ok(report) => {
                        cot.add_observation(format!("Security: {}", report));
                        if report.contains("CRITICAL") {
                            let msg = format!("Deployment blocked — security audit failed:\n{}", report);
                            cot.add_thought(
                                "Critical security issue — blocking deployment".to_string(),
                                Action::FinalAnswer { output: msg.clone() },
                                0.0,
                            );
                            return msg;
                        }

                        // Step 3: Deploy
                        let registry = std::env::var("REGISTRY")
                            .unwrap_or_else(|_| "ghcr.io/aetherclaw".to_string());
                        let deployer = specialized::DeployerAgent::new(self.tools.clone(), registry);
                        cot.add_thought(
                            "Dispatching Deployer Agent".to_string(),
                            Action::SpawnAgent {
                                agent_type: AgentType::Deployer,
                                task: format!("Deploy {}", artifact),
                            },
                            0.88,
                        );

                        match deployer.execute(&artifact, "latest").await {
                            Err(e) => {
                                cot.add_observation(format!("Deployment failed: {}", e));
                                return format!("Deployment failed: {}", e);
                            }
                            Ok(deploy_url) => {
                                cot.add_observation(format!("Deployed: {}", deploy_url));

                                // Step 4: Monitor
                                let monitor = specialized::MonitorAgent::new(
                                    format!("{}/api", deploy_url),
                                );
                                let health = monitor.execute(&deploy_url).await.unwrap_or_else(|e| {
                                    format!("Health check skipped: {}", e)
                                });
                                cot.add_observation(health.clone());

                                let final_output = format!(
                                    "Build & Deploy Complete\n\
                                    • Artifact: {}\n\
                                    • Security: {}\n\
                                    • Deployment: {}\n\
                                    • Health: {}",
                                    artifact,
                                    if report.contains("CRITICAL") { "Failed" } else { "Passed" },
                                    deploy_url,
                                    health
                                );
                                cot.add_thought(
                                    "Pipeline complete".to_string(),
                                    Action::FinalAnswer { output: final_output.clone() },
                                    0.98,
                                );
                                return final_output;
                            }
                        }
                    }
                }

                // Security skipped — deploy anyway
                format!("Built: {}\nSecurity audit skipped.", artifact)
            }
        }
    }

    async fn handle_standard_query(
        &self,
        msg: &InboundMessage,
        cot: &mut ChainOfThought,
        _session_id: &str,
    ) -> String {
        // Load recent conversation history for context
        let history = {
            let db = self.db.lock().await;
            db.get_history(&msg.session_key, 20).unwrap_or_default()
        };

        let system_ctx = "You are AetherClaw, an ultra-efficient edge AI assistant. \
            Respond concisely. When you need to use a tool, respond with JSON: \
            {\"action_type\": \"ToolCall\", \"action_details\": \"tool_name:input\"} \
            When done, respond with JSON: {\"action_type\": \"FinalAnswer\", \"action_details\": \"your answer\"} \
            or just respond naturally (non-JSON responses are treated as final answers).";

        let max_iterations = 8;
        let mut last_response = String::new();

        for _ in 0..max_iterations {
            let prompt = cot.build_prompt();

            match self.llm.infer_with_history(&prompt, Some(system_ctx), &history).await {
                Err(e) => {
                    error!("LLM inference error: {}", e);
                    return format!("I encountered an error processing your request: {}", e);
                }
                Ok(response) => {
                    last_response = response.clone();

                    match parse_action(&response) {
                        Some(Action::ToolCall { name, input }) => {
                            cot.add_thought(
                                format!("Calling tool: {}", name),
                                Action::ToolCall { name: name.clone(), input: input.clone() },
                                0.85,
                            );
                            let result = self.tools.execute(&name, &input).await;
                            cot.add_observation(result);
                        }
                        Some(Action::FinalAnswer { output }) => {
                            cot.add_thought(
                                "Providing final answer".to_string(),
                                Action::FinalAnswer { output: output.clone() },
                                1.0,
                            );
                            return output;
                        }
                        _ => {
                            // Non-JSON response → treat as final answer
                            cot.add_thought(
                                "Direct response".to_string(),
                                Action::FinalAnswer { output: response.clone() },
                                0.9,
                            );
                            return response;
                        }
                    }

                    if cot.is_complete() {
                        break;
                    }
                }
            }
        }

        cot.get_final_answer().unwrap_or(last_response)
    }

    async fn send_reply(&self, session_key: &str, content: &str, channel: String) {
        let outbound = OutboundMessage {
            channel,
            recipient: session_key.to_string(),
            content: content.to_string(),
            session_key: session_key.to_string(),
        };
        if let Err(e) = self.bus_tx.send(outbound).await {
            error!("Failed to send outbound message: {}", e);
        }
    }

    async fn save_cot_trace(&self, session_id: &str, trace: &str) {
        let _ = tokio::fs::create_dir_all(&self.cot_trace_dir).await;
        let path = self.cot_trace_dir.join(format!("{}.json", session_id));
        let _ = tokio::fs::write(path, trace).await;

        // Also persist to DB
        let db = self.db.lock().await;
        let _ = db.save_cot_trace(session_id, trace);
    }
}

fn parse_action(response: &str) -> Option<Action> {
    // Try JSON action format
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(response.trim()) {
        let action_type = json.get("action_type")?.as_str()?;
        let details = json.get("action_details")?.as_str()?;

        return match action_type {
            "ToolCall" => {
                let (name, input) = details.split_once(':')?;
                Some(Action::ToolCall {
                    name: name.to_string(),
                    input: input.to_string(),
                })
            }
            "FinalAnswer" => Some(Action::FinalAnswer { output: details.to_string() }),
            _ => None,
        };
    }
    None
}
