# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Higress is an AI-native API gateway built on Istio and Envoy. It is a Go project (Go 1.23) forked/extended from Istio, with Wasm plugin support in Go, Rust, C++, and AssemblyScript. The module path is `github.com/alibaba/higress/v2`.

## Build Commands

```bash
# Pre-development setup (REQUIRED before building or after cloning)
make prebuild && go mod tidy

# Build the higress controller binary
make build

# Build for Linux
make build-linux

# Build hgctl CLI tool
make build-hgctl

# Run unit tests with coverage
make go.test.coverage
# Which runs: go test ./cmd/... ./pkg/... -race -coverprofile=coverage.xml -covermode=atomic

# Run a single unit test
go test -v -run TestName ./pkg/path/to/package/...

# Lint
make lint

# Generate API code (requires container)
GENERATE_API=1 make gen-api

# Generate Kubernetes client code
make gen-client
```

## E2E Tests

E2E tests run in a Kind cluster and require Docker. They use Go build tags:

```bash
# Full e2e cycle: create cluster, build, load images, install, test, cleanup
make higress-conformance-test

# Run specific e2e test
make run-higress-e2e-test TEST_SHORTNAME=TestName

# Wasm plugin e2e tests
make higress-wasmplugin-test PLUGIN_TYPE=<type> PLUGIN_NAME=<name>
```

E2E test code lives in `test/e2e/` and uses the build tag `conformance`.

## Architecture

### Git Submodules

The project depends on forked Istio components as git submodules under `istio/` and `envoy/`. Run `make prebuild` (which calls `git submodule update --init`) before building. The `tools/hack/prebuild.sh` script copies submodule sources into `external/` for the build.

**Submodule repos** (all under `higress-group` GitHub org):
- `istio/istio`, `istio/api`, `istio/client-go`, `istio/pkg`, `istio/proxy` — forked Istio (branch `istio-1.19`)
- `envoy/envoy`, `envoy/go-control-plane` — forked Envoy (branch `envoy-1.27`)

### Key Directories

- **`cmd/higress/`** — Main controller entrypoint
- **`hgctl/`** — Separate Go module for the `hgctl` CLI tool (has its own `go.mod`)
- **`pkg/`** — Core Go packages:
  - `pkg/ingress/` — Ingress config, Kubernetes integration, MCP, translation logic
  - `pkg/bootstrap/` — Server bootstrap
  - `pkg/cert/` — Certificate management
  - `pkg/config/` — Configuration handling
- **`api/`** — Protobuf API definitions and generated code (Istio-based CRDs)
- **`client/`** — Generated Kubernetes client code
- **`plugins/`** — Wasm and golang-filter plugins:
  - `plugins/wasm-go/extensions/` — Go-based Wasm plugins (ai-proxy, ai-cache, cors, jwt-auth, etc.)
  - `plugins/wasm-rust/extensions/` — Rust-based Wasm plugins
  - `plugins/wasm-cpp/extensions/` — C++ Wasm plugins
  - `plugins/golang-filter/` — Native Go filter plugins (MCP server/session)
- **`external/`** — Copied from submodules during prebuild; do not edit directly
- **`helm/`** — Helm charts for Kubernetes deployment
- **`docker/`** — Dockerfiles and docker build config
- **`tools/hack/`** — Build scripts (gobuild.sh, prebuild.sh, build-envoy.sh, etc.)

### Versioning

- `VERSION` file contains the current version (e.g., `v2.1.11`)
- `DEP_VERSION` tracks console dependency version
- Version info is injected via Go ldflags at build time from `pkg/cmd/lversion`

### Docker Images

Images are hosted at `higress-registry.cn-hangzhou.cr.aliyuncs.com/higress/`. Key images: `higress` (controller), `pilot`, `gateway`, `all-in-one`.

## Commit Message Convention

Use prefixed messages: `docs:`, `feature:`, `bugfix:`, `refactor:`, `test:`.

## Plugin Development

New plugins require a `design/` directory containing design documentation. See `.cursor/rules/plugin-development.mdc` for full requirements. Each wasm-go plugin is a standalone Go module with its own `go.mod` under `plugins/wasm-go/extensions/<name>/`.
