use anyhow::Result;
use serde_json::Value;

#[derive(Clone)]
pub struct ModelRouter {
    local_engine: Option<LocalEngine>,
    cloud_clients: Vec<CloudClient>,
    pub default_model: String,
}

#[derive(Clone)]
struct LocalEngine {
    model_path: String,
}

#[derive(Clone)]
struct CloudClient {
    model_name: String,
    api_base: String,
    api_key: String,
    client: reqwest::Client,
}

impl ModelRouter {
    pub async fn new(config: crate::config::LlmConfig) -> Result<Self> {
        let local_engine = if config.local_models_path.exists() {
            let model_file = config.local_models_path.join(&config.default_local_model);
            if model_file.exists() {
                Some(LocalEngine {
                    model_path: model_file.to_string_lossy().to_string(),
                })
            } else {
                None
            }
        } else {
            None
        };

        let cloud_clients = config.model_list.into_iter().map(|entry| {
            CloudClient {
                model_name: entry.model_name,
                api_base: entry.api_base.unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
                api_key: entry.api_key.unwrap_or_default(),
                client: reqwest::Client::new(),
            }
        }).collect();

        Ok(Self {
            local_engine,
            cloud_clients,
            default_model: config.default_local_model,
        })
    }

    pub fn is_local_available(&self) -> bool {
        self.local_engine.is_some()
    }

    pub fn available_models(&self) -> Vec<String> {
        let mut models = vec![];
        if let Some(local) = &self.local_engine {
            models.push(format!("local:{}", local.model_path));
        }
        for c in &self.cloud_clients {
            models.push(c.model_name.clone());
        }
        models
    }

    pub async fn infer(&self, prompt: &str, context: Option<&str>) -> Result<String> {
        if let Some(local) = &self.local_engine {
            match self.local_infer(local, prompt, context).await {
                Ok(resp) => return Ok(resp),
                Err(e) => tracing::warn!("Local inference failed: {}, trying cloud...", e),
            }
        }

        if let Some(cloud) = self.cloud_clients.first() {
            self.cloud_infer(cloud, prompt, context).await
        } else {
            anyhow::bail!("No inference engine available. Configure a model in ~/.aetherclaw/config.toml")
        }
    }

    pub async fn infer_with_history(
        &self,
        prompt: &str,
        context: Option<&str>,
        history: &[(String, String)],
    ) -> Result<String> {
        if let Some(local) = &self.local_engine {
            match self.local_infer(local, prompt, context).await {
                Ok(resp) => return Ok(resp),
                Err(e) => tracing::warn!("Local inference failed: {}, trying cloud...", e),
            }
        }

        if let Some(cloud) = self.cloud_clients.first() {
            self.cloud_infer_with_history(cloud, prompt, context, history).await
        } else {
            anyhow::bail!("No inference engine available")
        }
    }

    async fn local_infer(&self, engine: &LocalEngine, prompt: &str, _context: Option<&str>) -> Result<String> {
        // llama-cpp-rs integration placeholder — loads model on first call
        // Feature-gated: enable with `--features local-inference` when llama-cpp-rs is configured
        tracing::info!("Local model path: {} (integration pending)", engine.model_path);
        anyhow::bail!("Local inference not yet compiled in. Add llama-cpp-rs feature.")
    }

    async fn cloud_infer(&self, client: &CloudClient, prompt: &str, context: Option<&str>) -> Result<String> {
        let messages = build_messages(context, &[], prompt);
        self.do_completion(client, messages).await
    }

    async fn cloud_infer_with_history(
        &self,
        client: &CloudClient,
        prompt: &str,
        context: Option<&str>,
        history: &[(String, String)],
    ) -> Result<String> {
        let messages = build_messages(context, history, prompt);
        self.do_completion(client, messages).await
    }

    async fn do_completion(&self, client: &CloudClient, messages: serde_json::Value) -> Result<String> {
        let res = client
            .client
            .post(format!("{}/chat/completions", client.api_base))
            .header("Authorization", format!("Bearer {}", client.api_key))
            .json(&serde_json::json!({
                "model": client.model_name,
                "messages": messages,
                "temperature": 0.7,
                "max_tokens": 2048
            }))
            .send()
            .await?;

        let status = res.status();
        let body = res.json::<Value>().await?;

        if !status.is_success() {
            let err = body["error"]["message"].as_str().unwrap_or("Unknown API error");
            anyhow::bail!("LLM API error {}: {}", status, err);
        }

        Ok(body["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("(empty response)")
            .to_string())
    }
}

fn build_messages(
    context: Option<&str>,
    history: &[(String, String)],
    prompt: &str,
) -> serde_json::Value {
    let mut messages = vec![];

    messages.push(serde_json::json!({
        "role": "system",
        "content": context.unwrap_or("You are AetherClaw, an ultra-efficient edge AI assistant. Be concise and precise.")
    }));

    for (role, content) in history {
        messages.push(serde_json::json!({
            "role": role,
            "content": content
        }));
    }

    messages.push(serde_json::json!({
        "role": "user",
        "content": prompt
    }));

    serde_json::Value::Array(messages)
}
