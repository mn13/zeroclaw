# Docker Deployment

ZeroClaw provides Docker images and a compose setup for deploying the full stack: a gateway with one or more agent instances.

## Images

### `Dockerfile.zcgw` — Gateway Image

Multi-stage build:
1. **Builder**: `rust:1.88-slim` with `protobuf-compiler`. Builds the `zcgw` binary with the `ci` profile.
2. **Runtime**: `debian:bookworm-slim` with `ca-certificates` and `docker.io` (for managing agent containers via the Docker socket).

- **Exposed port**: `8080`
- **Command**: `zcgw`
- Runs as root to access `/var/run/docker.sock`.

### `Dockerfile.zc` — Agent Image

Multi-stage build:
1. **Builder**: `rust:1.88-slim` with `protobuf-compiler`. Builds the `zc` binary with the `ci` profile.
2. **Runtime**: `debian:bookworm-slim` with `ca-certificates` and `curl`.

- **Exposed port**: `50051` (gRPC)
- **Volume**: `/data` (persistent history and memory)
- **Entrypoint**: `entrypoint-zc.sh`
- **Default command**: `zc --grpc-port 50051 --data-dir /data --config /etc/zc/config.toml`

## Docker Compose

The compose file (`docker/docker-compose.yml`) defines the gateway service:

```yaml
services:
  gateway:
    build:
      context: ..
      dockerfile: docker/Dockerfile.zcgw
    ports:
      - "8080:8080"
    environment:
      ZCGW_AUTH_TOKEN: "${ZCGW_AUTH_TOKEN:-test-token-123}"
      ZCGW_GRPC_SECRET: "${ZCGW_GRPC_SECRET:-grpc-secret-456}"
      ZCGW_CONFIG_PATH: /etc/zcgw/config.toml
      ZCGW_DOCKER_IMAGE: zeroclaw-agent
      ZCGW_DOCKER_NETWORK: "${ZCGW_DOCKER_NETWORK:-docker_default}"
      ZCGW_DOCKER_CONFIG_TEMPLATE: /etc/zcgw/zc-template.toml
      ZCGW_HOST_MODE: "false"
      ZCGW_AGENTS_DIR: /var/lib/zcgw/agents
      ZCGW_HOST_AGENTS_DIR: "${PWD}/agents"
      ZCGW_BASE_PORT: "50051"
      VENICE_API_KEY: "${VENICE_API_KEY:-}"
      OPENROUTER_API_KEY: "${OPENROUTER_API_KEY:-}"
      ANTHROPIC_API_KEY: "${ANTHROPIC_API_KEY:-}"
    volumes:
      - ./config/zcgw.toml:/etc/zcgw/config.toml
      - ./config/zc-venice.toml:/etc/zcgw/zc-template.toml:ro
      - ./agents:/var/lib/zcgw/agents
      - /var/run/docker.sock:/var/run/docker.sock
    deploy:
      resources:
        limits:
          memory: 256M
```

Note that `ZCGW_DOCKER_IMAGE` is set to `zeroclaw-agent` here, overriding the code default of `zeroclaw:latest`. Similarly, `ZCGW_DOCKER_NETWORK` defaults to `docker_default` in compose, overriding the code default of `zeroclaw-net`. The `OPENAI_API_KEY` is not explicitly listed in compose but is auto-forwarded by the gateway code if present in the environment.

Agent containers are **not** defined in compose — they are dynamically created and managed by the gateway at runtime via the Docker API.

## How It Works

### Startup Sequence

1. The gateway container starts and loads its configuration from `/etc/zcgw/config.toml`.
2. `ensure_agents_from_config` reconciles the desired state:
   - For each instance in the config, it checks if a Docker container exists.
   - Missing containers are created from the `ZCGW_DOCKER_IMAGE`.
   - Containers are started or stopped based on `desired_state`.
3. The health check loop starts, pinging all agent instances every 30 seconds.

### Agent Container Lifecycle

When creating a new agent container, the gateway:
1. Creates the agent's data directory: `<agents_dir>/<id>/`
2. Writes the agent config to `<agents_dir>/<id>/config.toml`
3. Creates a `<agents_dir>/<id>/data/` subdirectory for persistent storage
4. Runs `docker create` with:
   - The configured image (`ZCGW_DOCKER_IMAGE`)
   - Network: `ZCGW_DOCKER_NETWORK`
   - Memory limit: `ZCGW_DOCKER_MEMORY_LIMIT`
   - Port mapping: `<host_port>:50051`
   - Volume mounts for config and data
   - Environment variables (API keys forwarded from gateway)
   - Container name: `zc-<id>`
5. Starts the container with `docker start`

### Networking

| Mode | Gateway → Agent | Details |
|------|----------------|---------|
| **Docker mode** (`ZCGW_HOST_MODE=false`) | Via Docker network hostname | Agents are addressed as `http://zc-<id>:50051` on the shared Docker network. |
| **Host mode** (`ZCGW_HOST_MODE=true`) | Via localhost port mapping | Agents are addressed as `http://localhost:<port>`. Ports are assigned sequentially from `ZCGW_BASE_PORT`. |

### Volume Mounts

| Container Path | Host Path | Purpose |
|----------------|-----------|---------|
| `/etc/zcgw/config.toml` | `docker/config/zcgw.toml` | Gateway configuration |
| `/etc/zcgw/zc-template.toml` | `docker/config/zc-venice.toml` | Default agent config template |
| `/var/lib/zcgw/agents/` | `docker/agents/` | Agent data directories |
| `/var/run/docker.sock` | `/var/run/docker.sock` | Docker API access |

### Environment Variables

API key variables (`VENICE_API_KEY`, `OPENROUTER_API_KEY`, `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`) are automatically forwarded from the gateway's environment to newly created agent containers. Additional variables can be specified via `ZCGW_DOCKER_ENV_VARS`.

## Example Configuration

### Gateway Config (`docker/config/zcgw.toml`)

```toml
listen_addr = "0.0.0.0:8080"

[instances.agent-ava]
grpc_address = "http://agent-ava:50051"
display_name = "ava"
desired_state = "running"

[instances.agent-morph]
grpc_address = "http://agent-morph:50051"
display_name = "morpheus"
desired_state = "running"
```

### Running

```bash
# Set required environment variables
export ZCGW_AUTH_TOKEN="your-secret-token"
export OPENROUTER_API_KEY="sk-or-..."

# Build and start
cd docker
docker compose up --build -d

# Access the Web UI
open http://localhost:8080
```
