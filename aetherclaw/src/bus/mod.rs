use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InboundMessage {
    pub channel: String, // "telegram", "discord", "web"
    pub sender: String,
    pub content: String,
    pub session_key: String,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OutboundMessage {
    pub channel: String,
    pub recipient: String,
    pub content: String,
    pub session_key: String,
}

pub struct MessageBus {
    pub inbound_tx: mpsc::Sender<InboundMessage>,
    inbound_rx: Option<mpsc::Receiver<InboundMessage>>,
    pub outbound_tx: mpsc::Sender<OutboundMessage>,
    outbound_rx: Option<mpsc::Receiver<OutboundMessage>>,
}

impl MessageBus {
    pub fn new(capacity: usize) -> Self {
        let (inbound_tx, inbound_rx) = mpsc::channel(capacity);
        let (outbound_tx, outbound_rx) = mpsc::channel(capacity);

        Self {
            inbound_tx,
            inbound_rx: Some(inbound_rx),
            outbound_tx,
            outbound_rx: Some(outbound_rx),
        }
    }

    pub fn take_inbound_rx(&mut self) -> Option<mpsc::Receiver<InboundMessage>> {
        self.inbound_rx.take()
    }

    pub fn take_outbound_rx(&mut self) -> Option<mpsc::Receiver<OutboundMessage>> {
        self.outbound_rx.take()
    }
}
