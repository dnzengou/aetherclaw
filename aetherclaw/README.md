# 🦐 AetherClaw — Edge AI Command Center

AetherClaw is an ultra-lean, Rust-based AI agent system with Chain-of-Thought multi-agent orchestration. It's the spiritual successor to PicoClaw, designed for edge deployment with minimal resource footprint.

## Features

- **🧠 Chain-of-Thought Multi-Agent System**: Builder, Security, Deployer, and Monitor agents working together
- **⚡ Ultra-Lightweight**: <5MB RAM footprint, <500ms boot time
- **🔒 Security-First**: Sandboxed execution with cap-std
- **☁️ Hybrid LLM**: Local-first (Phi-2 Q4) with cloud fallback
- **📱 Multi-Channel**: Telegram, Discord, Web UI
- **🚀 Production-Ready**: Docker, Kubernetes, CI/CD

## Quick Start

```bash
# Clone and build
git clone https://github.com/your-org/aetherclaw.git
cd aetherclaw

# Build optimized binary
cargo build --release --target x86_64-unknown-linux-musl

# Run with TUI wizard
./target/release/aetherclaw

# Or deploy with Docker
docker-compose up -d
```

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    AetherClaw Core                          │
├─────────────┬─────────────┬─────────────┬──────────────────┤
│   Agent     │    LLM      │   Channels  │     Tools        │
│  Orchestra- │   Router    │  Telegram   ├─ Sandboxed FS    │
│    tor      │  Local/Cloud│   Discord   ├─ Command Exec    │
│  (CoT)      │             │    Web UI   ├─ Web Search      │
└─────────────┴─────────────┴─────────────┴──────────────────┘
```

## Multi-Agent Pipeline

```
User Request → Builder → Security Audit → Deployer → Monitor
                    ↓
              CoT Traces Saved
```

## Deployment

### Docker Compose (Local)
```bash
docker-compose -f docker-compose.prod.yml up -d
```

### Kubernetes
```bash
kubectl apply -f k8s/
```

### Multi-Arch Build
```bash
docker buildx build --platform linux/amd64,linux/arm64,linux/riscv64 \
  -t aetherclaw/aetherclaw:latest \
  -f Dockerfile.multiarch \
  --push .
```

## Configuration

Edit `~/.aetherclaw/config.toml`:

```toml
[gateway]
host = "0.0.0.0"
port = 8080

[llm]
default_local_model = "phi-2-q4.gguf"

[[llm.model_list]]
model_name = "gpt-4-mini"
model = "openai/gpt-4"
api_key = "sk-..."
```

## Web UI

Access the dashboard at `http://localhost:8080`

Features:
- Real-time chat with CoT visualization
- Deployment pipeline control
- System metrics monitoring
- Agent status tracking

## Commands

| Command | Description |
|---------|-------------|
| `build and deploy` | Full CI/CD pipeline |
| `security audit` | Run vulnerability scan |
| `optimize for <target>` | Cross-compile optimization |
| `status` | System health check |

## License

MIT License — 皮皮虾，我们走！🚀
