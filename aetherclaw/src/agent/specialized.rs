use super::cot::AgentType;
use crate::tools::ToolKit;
use std::process::Stdio;
use tokio::process::Command;
use tracing::{info, error};

pub struct BuilderAgent {
    tools: ToolKit,
    workspace: std::path::PathBuf,
}

impl BuilderAgent {
    pub fn new(tools: ToolKit, workspace: std::path::PathBuf) -> Self {
        Self { tools, workspace }
    }

    pub async fn execute(&self, task: &str) -> Result<String, String> {
        info!("BuilderAgent executing: {}", task);
        
        // Parse task for build parameters
        let target = if task.contains("musl") {
            "x86_64-unknown-linux-musl"
        } else if task.contains("arm") {
            "aarch64-unknown-linux-musl"
        } else {
            "x86_64-unknown-linux-gnu"
        };

        let profile = if task.contains("release") || task.contains("deploy") {
            "release"
        } else {
            "debug"
        };

        // Execute cargo build
        let output = Command::new("cargo")
            .args(&[
                "build",
                "--profile", profile,
                "--target", target,
                "--bin", "aetherclaw"
            ])
            .current_dir(&self.workspace)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| format!("Build command failed: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Build failed: {}", stderr));
        }

        // Run tests
        let test_output = Command::new("cargo")
            .args(&["test", "--release", "--", "--nocapture"])
            .current_dir(&self.workspace)
            .output()
            .await
            .map_err(|e| format!("Test command failed: {}", e))?;

        if !test_output.status.success() {
            return Err("Tests failed".to_string());
        }

        // Get binary size
        let binary_path = self.workspace
            .join("target")
            .join(target)
            .join(profile)
            .join("aetherclaw");

        let metadata = tokio::fs::metadata(&binary_path)
            .await
            .map_err(|e| format!("Failed to read binary metadata: {}", e))?;

        let size_mb = metadata.len() as f64 / 1_048_576.0;
        
        Ok(format!(
            "Build successful: {} ({:.2} MB)\nTarget: {}\nProfile: {}\nTests: Passed",
            binary_path.display(),
            size_mb,
            target,
            profile
        ))
    }
}

pub struct SecurityAgent {
    tools: ToolKit,
}

impl SecurityAgent {
    pub fn new(tools: ToolKit) -> Self {
        Self { tools }
    }

    pub async fn execute(&self, target: &str) -> Result<String, String> {
        info!("SecurityAgent auditing: {}", target);
        
        let mut findings = vec![];
        
        // Check for secrets in binary
        let strings_output = Command::new("strings")
            .arg(target)
            .output()
            .await
            .map_err(|e| format!("Strings command failed: {}", e))?;
            
        let content = String::from_utf8_lossy(&strings_output.stdout);
        
        let suspicious_patterns = ["password", "secret", "api_key", "private_key", "token"];
        for pattern in &suspicious_patterns {
            if content.to_lowercase().contains(pattern) {
                findings.push(format!("⚠️  Potential {} found in binary strings", pattern));
            }
        }

        // Check dependencies with cargo-audit (if available)
        let audit_output = Command::new("cargo")
            .args(&["audit", "--json"])
            .current_dir(std::path::Path::new(target).parent().unwrap_or(std::path::Path::new(".")))
            .output()
            .await;

        if let Ok(output) = audit_output {
            if !output.status.success() {
                findings.push("❌ Cargo audit found vulnerabilities in dependencies".to_string());
            } else {
                findings.push("✅ No known vulnerabilities in dependencies".to_string());
            }
        }

        if findings.is_empty() {
            Ok("✅ Security audit passed. No issues found.".to_string())
        } else {
            Ok(findings.join("\n"))
        }
    }
}

pub struct DeployerAgent {
    tools: ToolKit,
    registry: String,
}

impl DeployerAgent {
    pub fn new(tools: ToolKit, registry: String) -> Self {
        Self { tools, registry }
    }

    pub async fn execute(&self, artifact: &str, version: &str) -> Result<String, String> {
        info!("DeployerAgent deploying: {} v{}", artifact, version);
        
        // Build Docker image
        let tag = format!("{}/aetherclaw:{}", self.registry, version);
        
        let build_output = Command::new("docker")
            .args(&[
                "build", 
                "-t", &tag,
                "--build-arg", &format!("BINARY={}", artifact),
                "."
            ])
            .output()
            .await
            .map_err(|e| format!("Docker build failed: {}", e))?;

        if !build_output.status.success() {
            return Err("Docker build failed".to_string());
        }

        // Push to registry
        let push_output = Command::new("docker")
            .args(&["push", &tag])
            .output()
            .await
            .map_err(|e| format!("Docker push failed: {}", e))?;

        if !push_output.status.success() {
            return Err("Docker push failed".to_string());
        }

        // Update docker-compose deployment
        let deploy_output = Command::new("docker-compose")
            .args(&["-f", "docker-compose.prod.yml", "up", "-d", "--no-deps", "aetherclaw"])
            .output()
            .await
            .map_err(|e| format!("Deployment failed: {}", e))?;

        if !deploy_output.status.success() {
            return Err("Deployment failed".to_string());
        }

        Ok(format!(
            "Deployed successfully\n• Image: {}\n• Registry: {}\n• Status: Active",
            tag, self.registry
        ))
    }
}

pub struct MonitorAgent {
    health_endpoint: String,
}

impl MonitorAgent {
    pub fn new(health_endpoint: String) -> Self {
        Self { health_endpoint }
    }

    pub async fn execute(&self, deployment_url: &str) -> Result<String, String> {
        info!("MonitorAgent checking: {}", deployment_url);
        
        // Perform health checks
        let client = reqwest::Client::new();
        
        // Check /health
        let health_res = client
            .get(format!("{}/health", deployment_url))
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await;
            
        match health_res {
            Ok(res) if res.status().is_success() => {
                let latency = res.headers()
                    .get("x-response-time")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("unknown");
                    
                Ok(format!(
                    "✅ Health check passed\n• Endpoint: {}/health\n• Latency: {}\n• Status: 200 OK",
                    deployment_url, latency
                ))
            }
            Ok(res) => Err(format!("Health check returned status: {}", res.status())),
            Err(e) => Err(format!("Health check failed: {}", e)),
        }
    }
}
