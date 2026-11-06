# Workflows

## Runners

All Linux jobs run on GitHub-hosted Ubuntu 24.04 runners:

```yaml
runs-on: ubuntu-24.04
```

| Runner | Used by |
|---|---|
| `ubuntu-24.04` | Linux CI (`fmt`, `lint`, `test`), bots, scripts, release builds, auto-fix / index-publish |
| `ubuntu-24.04-arm` | native aarch64-linux release builds |
| `macos-latest` | macOS arm64 release builds |

## Caching

`Swatinem/rust-cache@v2` covers the dependency `target/` + `~/.cargo` blobs.

## History

This repo previously ran Linux CI on a self-hosted Hetzner fleet. That fleet was
retired along with its fleet-maintenance workflows.
