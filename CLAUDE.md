# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Higress is a cloud-native AI Gateway built on Istio and Envoy. It provides AI gateway capabilities (LLM providers, MCP server hosting), Kubernetes ingress with Gateway API support, microservice gateway with service discovery (Nacos, Consul, Eureka, etc.), and extensibility via Wasm plugins (Go/Rust/C++/AssemblyScript) and golang-filter plugins.

## Core Architecture

Three main components:

1. **Higress Controller** (control plane) - Entry point: `cmd/higress/main.go` → `pkg/cmd`
   - **Discovery**: Istio Pilot-Discovery for service discovery and config management
   - **Higress Core** (`pkg/ingress/kube/`): 6 controllers — Ingress, Gateway, McpBridge, Http2Rpc, WasmPlugin, ConfigmapMgr

2. **Higress Gateway** (data plane) - Envoy with Pilot Agent proxying xDS

3. **Higress Console** - Web UI (separate repo: higress-group/higress-console)

Config flow: Ingress/Gateway API → Higress Core → MCP over xDS → Discovery → xDS → Envoy

## Build Commands

Go version: 1.24+ (module requires 1.24.4)

```bash
# Prerequisites (required before any build)
make prebuild && go mod tidy

# Build controller
make build                # current platform
make build-linux          # linux

# Build hgctl CLI
make build-hgctl          # current platform
make build-hgctl-multiarch  # linux+windows amd64/arm64

# Docker
make docker-build         # build image
make docker-buildx-push   # multi-arch build+push
```

Build outputs go to `out/` or `out/linux_<arch>/`.

## Linting

```bash
make lint                 # all linters (golangci-lint, yamllint, codespell, shellcheck)
make lint.golint          # Go only
make lint.yamllint        # YAML only
make lint.codespell       # spell check
make lint.shellcheck      # shell scripts in tools/hack/
```

Go lint config: `tools/linter/golangci-lint/.golangci.yml`. Import ordering: stdlib → third-party → `github.com/alibaba/higress/` (enforced by goimports).

## Testing

### Unit Tests
```bash
make go.test.coverage                          # all unit tests with coverage
go test ./pkg/ingress/... -run TestSpecific    # single test/package
```

### E2E Conformance Tests (requires kind cluster + Docker)
```bash
make higress-conformance-test           # full cycle: create cluster → build → test → cleanup
make higress-conformance-test-prepare   # prepare env only (reusable)
make run-higress-e2e-test               # run tests only (after prepare)
TEST_SHORTNAME=TestName make run-higress-e2e-test  # specific test

make higress-conformance-test-clean     # cleanup
```

E2E tests live in `test/e2e/conformance/tests/` as pairs of `.go` (test logic) + `.yaml` (K8s manifests).

### Wasm Plugin Tests
```bash
PLUGIN_NAME=request-block make higress-wasmplugin-test              # specific Go plugin
PLUGIN_TYPE=CPP PLUGIN_NAME=key_auth make higress-wasmplugin-test   # C++ plugin
PLUGIN_TYPE=RUST PLUGIN_NAME=request-block make higress-wasmplugin-test  # Rust plugin
TEST_SHORTNAME=TestName make higress-wasmplugin-test                # specific test names
PLUGIN_NAME=ip-restriction make higress-wasmplugin-test-skip-docker-build  # skip rebuild
```

## Plugin Development

### Building Wasm Plugins (Go)
```bash
cd plugins/wasm-go
PLUGIN_NAME=request-block make build       # build specific plugin
PLUGIN_NAME=request-block make build-push  # build and push image

# Manual build alternative
cd extensions/<plugin-name>
GOOS=wasip1 GOARCH=wasm go build -buildmode=c-shared -o main.wasm .
```

### Plugin Locations
- `plugins/wasm-go/extensions/` - Go Wasm plugins (~56 plugins including ai-proxy, ai-cache, etc.)
- `plugins/wasm-cpp/extensions/` - C++ Wasm plugins
- `plugins/wasm-rust/extensions/` - Rust Wasm plugins
- `plugins/wasm-assemblyscript/extensions/` - AssemblyScript Wasm plugins
- `plugins/golang-filter/` - Go filter plugins (compiled differently from Wasm, includes mcp-server/mcp-session)

### New Plugin Requirements (CRITICAL)

When creating **new independent plugins**, you **MUST**:

1. Create a `design/` directory with `design-doc.md` (purpose, features, config params, test strategy) and `ai-prompts.md` (if using AI tools)
2. Include AI coding summary in PR description
3. PRs without design docs have **lower review priority**

See `.cursor/rules/plugin-development.mdc` for full standards and templates.

## Key Directories

- `cmd/higress/` - Controller entry point
- `pkg/ingress/kube/` - Core controllers (ingress, gateway, mcpbridge, http2rpc, wasmplugin, configmap)
- `pkg/bootstrap/` - Server bootstrap and startup
- `hgctl/` - CLI tool source
- `plugins/` - All plugin implementations
- `api/` - API definitions (protobuf, CRDs)
- `test/e2e/` - E2E conformance tests
- `helm/` - Helm charts
- `istio/`, `envoy/` - Git submodules (upstream deps)
- `external/` - Generated/extracted code from submodules (created by `make prebuild`)

## Development Workflow

### Submodules
Higress depends on Istio and Envoy as submodules. Always run `make prebuild` after pulling changes that affect submodules. This populates `external/` with code extracted from `istio/` and `envoy/`.

### Committing Changes
- Commit message prefixes: `docs:`, `feature:`, `bugfix:`, `refactor:`, `test:`
- Add change description to `changes/X.X.X.md`
- New plugins must include `design/` directory

## References

- Docs: https://higress.cn/ (CN) / https://higress.io/ (EN)
- Architecture: https://higress.cn/docs/latest/dev/architecture/
- Wasm plugin guide: https://higress.io/docs/latest/user/wasm-go/
- Contributing: CONTRIBUTING_EN.md / CONTRIBUTING_CN.md
