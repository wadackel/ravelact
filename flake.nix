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
          src = craneLib.cleanCargoSource ./.;
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
        apps.default = {
          type = "app";
          program = "${ravelact}/bin/ravelact";
        };
        apps.ravelact = {
          type = "app";
          program = "${ravelact}/bin/ravelact";
        };

        packages.default = ravelact;
        packages.ravelact = ravelact;

        devShells.default = craneLib.devShell {
          inputsFrom = [ ravelact ];
          packages = with pkgs; [
            just
            jq
            actionlint
            cargo-llvm-cov
          ];
        };
      });
}
