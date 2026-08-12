# Workflows

## Runners

All Linux jobs run on [Blacksmith](https://docs.blacksmith.sh):

```yaml
runs-on: blacksmith-8vcpu-ubuntu-2404
```

Sizing convention:

| Size | Used by |
|---|---|
| `blacksmith-2vcpu-ubuntu-2404` | bots, scripts, `fmt`, `scope`, release-assembly jobs |
| `blacksmith-4vcpu-ubuntu-2404` | `smoke` (CLI + MCP stdio) |
| `blacksmith-8vcpu-ubuntu-2404` | `lint`, `test`, release builds, auto-fix / index-publish |
| `blacksmith-8vcpu-ubuntu-2404-arm` | native aarch64-linux release builds |
| `blacksmith-16vcpu-ubuntu-2404` | `bench` (criterion + divan) |

**macOS** release legs stay on GitHub-hosted `macos-latest`.

If jobs sit in **Queued** forever, check the Blacksmith app is installed on the
org at <https://app.blacksmith.sh> — an unrecognised `blacksmith-*` label never
resolves and the job waits indefinitely.

## Caching

Blacksmith transparently redirects `actions/cache`, `Swatinem/rust-cache`, and
the `setup-*` actions to a cache colocated with the runners. No configuration
changes are needed.

`sccache` is deliberately **not** used. Per Blacksmith's docs it is one of the
few backends still routed to GitHub's cross-datacenter cache, so on these
runners it costs more than it saves. `Swatinem/rust-cache` alone covers the
dependency `target/` + `~/.cargo` blobs.

## History

This repo previously ran Linux CI on a self-hosted Hetzner fleet. That fleet is
gone. Removed along with it: `runner-health.yml`, `runner-gc.yml`,
`provision-runner-volume.yml`, `provision-postgres.yml`, the `disk-guard`
composite action, the mold→wild→default-ld fallback ladder, the ENOSPC
best-effort apt guards, and `bench.yml`'s OVHCloud provision/teardown jobs.
Those all existed to work around a disk-pressured, hand-maintained fleet;
ephemeral microVMs with 80+ GB scratch make them unnecessary.
