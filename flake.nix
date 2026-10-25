{
  description = "crabcc dev environment — Rust toolchain + the .tools CLI fleet + agent tooling (rtk) from llm-agents.nix";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    llm-agents.url = "github:numtide/llm-agents.nix";
    # crane: Rust package builder with incremental dep caching
    crane.url = "github:ipetkov/crane";
    # fenix: nightly Rust toolchains (matches rust-toolchain.toml channel = "nightly")
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      llm-agents,
      crane,
      fenix,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };
        lib = pkgs.lib;

        # Agent tooling from llm-agents.nix. `or { }` keeps the flake evaluating
        # on a system the upstream flake doesn't build for; the `?` guards below
        # only pull a package when it's actually exposed.
        agents = llm-agents.packages.${system} or { };

        # The .tools fleet, sourced from nixpkgs (a few attrs differ from the
        # binary name: task -> go-task, yq -> yq-go).
        toolFleet = with pkgs; [
          ripgrep
          fd
          bat
          eza
          delta
          sd
          choose
          dust
          procs
          bottom
          zoxide
          fzf
          jq
          yq-go
          xh
          hyperfine
          tokei
          hexyl
          watchexec
          ast-grep
          tree-sitter
          qsv
          dasel
          gh
          lazygit
          go-task
          sccache
          bun
          uv
        ];

        # Rust toolchain (from nixpkgs-unstable; track the crate MSRV in
        # Cargo.toml — bump the channel if a dep outruns it).
        rust = with pkgs; [
          rustc
          cargo
          clippy
          rustfmt
          rust-analyzer
        ];

        # Native build deps: clang + mold for the workspace's configured linker;
        # cmake + pkg-config for C-backed crates (e.g. aws-lc-rs via reqwest).
        buildDeps = with pkgs; [
          clang
          mold
          cmake
          pkg-config
        ];

        # rtk is crabcc's optional rewrite-chain stage (declared in .tools);
        # pull it from llm-agents.nix when available.
        agentTools = lib.optional (agents ? rtk) agents.rtk;

        # crane + fenix are still declared as flake inputs but are no longer
        # used: their only consumer was the wormhole build, whose crates were
        # trimmed out of the workspace in #767. Drop both inputs with the next
        # `nix flake lock` — keeping them costs a fetch, not a build.
      in
      {
        # ── packages ────────────────────────────────────────────────────────────
        # No workspace member is currently exposed as a Nix package; the
        # wormhole-relay / wormhole-node packages went away with the trim.
        packages = { };

        # ── CI checks ───────────────────────────────────────────────────────────
        # `nix flake check` still evaluates packages and the devShell below.
        # Building `-p wormhole-node` here is what kept this check red on main:
        # that crate is no longer a workspace member, so cargo aborted with
        # "package ID specification `wormhole-node` did not match any packages".
        checks = { };

        # ── dev shell (unchanged) ────────────────────────────────────────────────
        devShells.default = pkgs.mkShell {
          packages = rust ++ buildDeps ++ toolFleet ++ agentTools;
          shellHook = ''
            echo "crabcc devShell: rust $(rustc --version 2>/dev/null | cut -d' ' -f2) + the .tools fleet."
            echo "Build/test: task   |   fast build: task build-fast"
          '';
        };
      }
    );
}
