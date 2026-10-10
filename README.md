# crabcc

**Symbol index for AI coding agents.**

4412× faster than grep. 85% fewer bytes sent to the LLM. MCP-native.

## Quick start

```bash
# Install
curl -fsSL https://raw.githubusercontent.com/8b-is/crabcc/main/install.sh | bash

# Index a repo
cd your-project
crabcc index

# Query
crabcc lookup sym ParseError        # find symbol definition
crabcc lookup refs ParseError       # find all references
crabcc lookup outline src/main.rs   # structural outline

# MCP server (for AI agent tools)
crabcc serve
```

## Core commands

| Command | What |
|---------|------|
| `crabcc index` | Build symbol database (~5-30s for monorepo) |
| `crabcc lookup sym <name>` | Find symbol definition |
| `crabcc lookup refs <name>` | Find all references |
| `crabcc lookup outline <file>` | Structural outline |
| `crabcc graph` | Call-graph operations |
| `crabcc memory` | AI memory (past findings) |
| `crabcc serve` | MCP server + localhost viewer |

## Internal agents (per-crate specialists)

`crabcc agent --profile internal/<name>` loads a per-crate agent profile
from `internal_agents/`. Each profile pairs a TOML config (model,
max-iterations, env, gate commands) with a Markdown system prompt that
specialises an agent for one crate. Five profiles ship today:

| Profile | Owns | Bench gates |
|---|---|---|
| `internal/crabcc-core`   | indexing + storage + query primitives | `task bench-compress`, `task bench` |
| `internal/crabcc-cli`    | clap surface + agent runtime + manager | — |
| `internal/crabcc-mcp`    | stdio MCP server + tool surface | `cargo bench -p crabcc-mcp` |
| `internal/crabcc-memory` | AI memory layer + miners + embedders | `task memory-bench` |
| `internal/crabcc-viz`    | localhost dashboard + sync HTTP | — |

Each profile pulls in `internal_agents/shared.agent.md` as a preamble
(rebase-first workflow, lint/test/PR contract, manager coordination).
Missing profiles error hard with the available list — typos never
silently fall through to "no profile". Profile env (default
`CRABCC_BUILD_PROFILE=release-native`) is exported to the spawned
agent's child process.

```bash
# Drive a single crate-specialist on the host:
crabcc agent --profile internal/crabcc-core --run "audit hot loops in extract.rs"

# Per-crate repomix bundle (much smaller than full repo) for the agent:
task repomix-crate CRATE=crabcc-core
```

### Running all five in parallel via Apple `container`

[Apple's `container`](https://github.com/apple/container) is a Mac-native
OCI runtime backed by Virtualization.framework — drop-in for Docker on
arm64. The bundled `install/internal-agents/{Containerfile,compose.yml}`
work with both.

```bash
# 1. Install Apple container (one-time, macOS 15+):
brew install apple/container/container          # Homebrew tap
# Or from the official release: https://github.com/apple/container/releases

# 2. Build the image (uses the BuildKit-style Containerfile):
container build -t crabcc-internal-agents:dev \
    -f install/internal-agents/Containerfile .

# 3. Bring up all five per-crate agents:
container compose -f install/internal-agents/compose.yml up

# 4. Stream one agent's logs:
container compose -f install/internal-agents/compose.yml logs -f crabcc-core

# 5. Bash into a running agent:
container exec -it crabcc-agent-core bash

# 6. Tear down:
container compose -f install/internal-agents/compose.yml down
```

Same compose file works under Docker / Podman without changes — every
service entry is OCI-compatible, no Apple-specific keys.

### Verifying agents work inside the container

The `Containerfile` bakes in `cargo`, `task`, `gh`, `repomix`, plus a
`CARGO_HOME=/cargo` named volume so agents share dep compiles
(~5× faster on a warm cache). Quick smoke:

```bash
container run --rm \
    -v "$PWD":/workspace \
    -e CRABCC_PROFILE=internal/crabcc-core \
    crabcc-internal-agents:dev \
    "noop"           # runs `crabcc agent --profile internal/crabcc-core --run noop`
```

A successful run prints `crabcc agent: id=<hex>` and writes a row to
`~/.crabcc/_internal.db` (host-visible because the repo is bind-mounted).

#### Useful run-time flags

Apple `container`'s VMs run with a default 1 GiB / 4 CPU envelope and a
locked-down capability set; for resource-intensive agent work you can
override per-invocation. The bundled `compose.yml` already sets
`mem_limit: 4g` + `cpus: 4` + `cap_drop: ALL` + the SSH-agent passthrough
on every service; the table below is the equivalent for ad-hoc
`container run`:

| Flag | What it does |
|---|---|
| `--memory 32g --cpus 8` | Override the per-VM ceiling (default 1 GiB / 4 CPU). For repo-wide refactors against the memory crate's bench harness. |
| `--init` | Runs the lightweight init as PID 1 — forwards signals + reaps zombies. Always pass this for agents that spawn cargo/rustc/gh. |
| `--ssh` | Mounts `${SSH_AUTH_SOCK}` at `/run/host-services/ssh-auth.sock`. Survives logout/login — `git clone git@github.com:...` Just Works. |
| `--volume $PWD:/workspace` | Repo root bind-mount. RW; backup loop reads from here. |
| `--cap-drop ALL --cap-add CHOWN --cap-add DAC_OVERRIDE` | Minimum-viable cap set for agent edits. Compose has these. |
| `--publish 127.0.0.1:7878:7878` | Forward the live dashboard port if you `crabcc serve` inside the agent VM. |
| `--rm` | Don't accumulate stopped containers. Always pass for short-lived agents. |

Resource-heavy build of the image itself? Bump the builder VM:

```bash
container builder stop && container builder delete
container builder start --cpus 8 --memory 16g
container build -t crabcc-internal-agents:dev -f install/internal-agents/Containerfile .
```

#### Resource snapshot

```bash
container stats                              # all running, top-style
container stats crabcc-agent-core            # one specific
container stats --no-stream --format json    # one-shot JSON for scripting
```

#### Logs

```bash
container compose -f install/internal-agents/compose.yml logs -f crabcc-core
container logs crabcc-agent-core              # alt ad-hoc form
container logs --boot crabcc-agent-core       # VM boot + init dmesg (debugging)
```

#### Inspect

```bash
container ls --format json --all | jq
container inspect crabcc-agent-core | jq
container image inspect crabcc-internal-agents:dev | jq
```

## Architecture

> **Prefer diagrams?** See [`docs/OVERVIEW.md`](./docs/OVERVIEW.md) for the full Mermaid map (indexing, query router, memory RRF, integrations). This section keeps ASCII traces for copy-paste debugging.

```
6 crates:
  crabcc-cli     — CLI binary
  crabcc-compact — memory compaction
  crabcc-core    — indexing engine (tree-sitter based)
  crabcc-fetch   — URL fetch + crawl
  crabcc-mcp     — MCP server
  crabcc-memory  — AI memory (BM25 + sqlite-vec)
```

## Install options

```bash
brew install 8b-is/tap/crabcc
cargo install crabcc-cli
# or: curl -fsSL https://raw.githubusercontent.com/8b-is/crabcc/main/install.sh | bash
```

## License

MIT
