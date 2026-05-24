{
  description = "ravelact — static analysis CLI for GitHub Actions workflow estates";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, crane }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };

        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        commonArgs = {
          # Custom source filter: keep everything craneLib.cleanCargoSource
          # keeps (Cargo / Rust sources) AND anything under ./web so
          # rust-embed can pick up the SPA assets at build time, EXCEPT for
          # generated / external directories (web/dist, web/node_modules,
          # web/.vite) which are either gitignored build output or pnpm-
          # managed dependency caches. Excluding them keeps the Nix source
          # hash stable and avoids polluting the sandbox.
          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter = path: type:
              let
                p = toString path;
                webRoot = toString ./web;
                isUnderWeb = pkgs.lib.hasPrefix (webRoot + "/") p || p == webRoot;
                isExcluded =
                  pkgs.lib.hasPrefix (toString ./web/dist) p
                  || pkgs.lib.hasPrefix (toString ./web/node_modules) p
                  || pkgs.lib.hasPrefix (toString ./web/.vite) p;
              in
                (craneLib.filterCargoSources path type)
                || (isUnderWeb && !isExcluded);
          };
          strictDeps = true;
          # Tests rely on tests/fixtures/** which cleanCargoSource excludes.
          # Test execution is owned by `just ci` inside the dev shell, not by
          # `nix build`; the package output is build-only.
          doCheck = false;
          buildInputs = pkgs.lib.optionals pkgs.stdenv.isDarwin [
            pkgs.libiconv
          ];
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        ravelact = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          meta.mainProgram = "ravelact";
        });
      in
      {
        # `packages.default` / `apps.default` are intentionally omitted in
        # this revision: rust-embed requires `web/dist/` to exist at compile
        # time, but the Nix sandbox cannot run `pnpm install && vite build`
        # without additional `pnpm.fetchDeps`-style infrastructure. The
        # canonical build path for this project is now
        # `nix develop -c just build-release`, which chains `just frontend`
        # (pnpm + vite) before `cargo build`. A future plan will restore
        # `packages.default` via a dedicated frontend derivation.

        devShells.default = craneLib.devShell {
          inputsFrom = [ ravelact ];
          packages = with pkgs; [
            just
            jq
            actionlint
            zizmor
            cargo-llvm-cov
            nodejs_22
            pnpm_11
            oxipng
            buf
            protobuf
          ];
        };
      });
}
