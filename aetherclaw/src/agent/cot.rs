use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThoughtStep {
    pub step_number: u8,
    pub thought: String,
    pub action: Action,
    pub observation: Option<String>,
    pub confidence: f32, // 0.0 - 1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Action {
    ToolCall { name: String, input: String },
    SpawnAgent { agent_type: AgentType, task: String },
    Think { reasoning: String },
    FinalAnswer { output: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentType {
    Builder,      // Handles compilation, testing
    Deployer,     // Handles Docker/k8s deployment
    Security,     // Audits security before deployment
    Monitor,      // Post-deployment health checks
    Optimizer,    // Performance optimization
}

pub struct ChainOfThought {
    max_steps: u8,
    current_step: u8,
    reasoning_chain: VecDeque<ThoughtStep>,
    context: String,
}

impl ChainOfThought {
    pub fn new(max_steps: u8, context: String) -> Self {
        Self {
            max_steps,
            current_step: 0,
            reasoning_chain: VecDeque::with_capacity(max_steps as usize),
            context,
        }
    }

    pub fn add_thought(&mut self, thought: String, action: Action, confidence: f32) -> &mut Self {
        self.current_step += 1;
        let step = ThoughtStep {
            step_number: self.current_step,
            thought,
            action,
            observation: None,
            confidence,
        };
        self.reasoning_chain.push_back(step);
        self
    }

    pub fn add_observation(&mut self, observation: String) -> &mut Self {
        if let Some(last) = self.reasoning_chain.back_mut() {
            last.observation = Some(observation);
        }
        self
    }

    pub fn build_prompt(&self) -> String {
        let mut prompt = format!("Context: {}\n\nReasoning Chain:\n", self.context);
        
        for step in &self.reasoning_chain {
            prompt.push_str(&format!(
                "Step {} [Confidence: {:.0}%]: {}\nAction: {:?}\n",
                step.step_number,
                step.confidence * 100.0,
                step.thought,
                step.action
            ));
            
            if let Some(obs) = &step.observation {
                prompt.push_str(&format!("Observation: {}\n", obs));
            }
            prompt.push('\n');
        }

        prompt.push_str(&format!(
            "Step {}: Based on the above reasoning, what is your next action?\n\
            Options: [ToolCall], [SpawnAgent <type>], [Think], or [FinalAnswer]\n\
            Respond in JSON format with 'thought', 'action_type', and 'action_details'.",
            self.current_step + 1
        ));
        
        prompt
    }

    pub fn is_complete(&self) -> bool {
        self.current_step >= self.max_steps || 
        matches!(self.reasoning_chain.back().map(|s| &s.action), Some(Action::FinalAnswer { .. }))
    }

    pub fn get_final_answer(&self) -> Option<String> {
        self.reasoning_chain.back().and_then(|s| match &s.action {
            Action::FinalAnswer { output } => Some(output.clone()),
            _ => None,
        })
    }

    pub fn export_trace(&self) -> String {
        serde_json::to_string_pretty(&self.reasoning_chain).unwrap_or_default()
    }
    
    #[allow(dead_code)]
    pub fn get_chain(&self) -> &VecDeque<ThoughtStep> {
        &self.reasoning_chain
    }
}
