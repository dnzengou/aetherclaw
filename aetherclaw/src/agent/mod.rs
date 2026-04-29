use crate::bus::{InboundMessage, OutboundMessage};
use crate::llm::ModelRouter;
use crate::tools::ToolKit;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::info;

pub mod cot;
pub mod orchestrator;
pub mod specialized;

use orchestrator::MultiAgentOrchestrator;

pub struct AgentLoop {
    orchestrator: MultiAgentOrchestrator,
    _tools: ToolKit,
}

impl AgentLoop {
    pub fn new(
        llm: ModelRouter,
        tools: ToolKit,
        bus_tx: mpsc::Sender<OutboundMessage>,
        workspace: &std::path::Path,
        db: Arc<tokio::sync::Mutex<crate::tools::persistence::Database>>,
    ) -> Self {
        let orchestrator = MultiAgentOrchestrator::new(llm, tools.clone(), bus_tx, workspace, db);
        Self {
            orchestrator,
            _tools: tools,
        }
    }

    pub async fn run(self, inbound_rx: &mut mpsc::Receiver<InboundMessage>, cancel: CancellationToken) {
        info!("AgentLoop started (Multi-Agent CoT/ReAct)");

        loop {
            tokio::select! {
                Some(msg) = inbound_rx.recv() => {
                    let mut orch = self.orchestrator.clone();
                    tokio::spawn(async move {
                        orch.handle_request(msg).await;
                    });
                }
                _ = cancel.cancelled() => {
                    info!("AgentLoop shutting down...");
                    break;
                }
                else => break,
            }
        }
    }
}
