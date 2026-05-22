# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.7](https://github.com/wadackel/ravelact/compare/v0.0.6...v0.0.7) - 2026-05-22

### Fixed

- *(release-plz)* commit `web/dist/.gitkeep` placeholder so `cargo package --verify` no longer fails when release-plz copies the workspace into a temp worktree that respects `.gitignore` ([#17](https://github.com/wadackel/ravelact/pull/17))

### Other

- *(ci)* drop the `devshell-run` composite action; each Nix-using CI job now calls `setup-nix-devshell` once ([#16](https://github.com/wadackel/ravelact/pull/16))
- *(just)* split `frontend` recipe into `frontend-deps` (install only) and `frontend` (deps + build); CI format job skips the unnecessary vite build ([#17](https://github.com/wadackel/ravelact/pull/17))
- *(web)* restructure UI primitives under `web/src/ui/components/ui/` ([#15](https://github.com/wadackel/ravelact/pull/15))

## [0.0.6](https://github.com/wadackel/ravelact/compare/v0.0.5...v0.0.6) - 2026-05-21

### Added

- *(browse)* introduce SPA browse subcommand and release pipeline ([#14](https://github.com/wadackel/ravelact/pull/14))

## [0.0.4](https://github.com/wadackel/ravelact/compare/v0.0.3...v0.0.4) - 2026-05-09

### Fixed

- *(ci)* publish immutable release with assets ([#9](https://github.com/wadackel/ravelact/pull/9))

## [0.0.3](https://github.com/wadackel/ravelact/compare/v0.0.2...v0.0.3) - 2026-05-09

### Other

- *(release)* enable trusted publishing ([#7](https://github.com/wadackel/ravelact/pull/7))

## [0.0.2](https://github.com/wadackel/ravelact/compare/v0.0.1...v0.0.2) - 2026-05-09

### Other

- *(ci)* add renovate config
- *(devshell)* add command wrapper action ([#5](https://github.com/wadackel/ravelact/pull/5))
- *(release)* add crates.io publish workflow ([#4](https://github.com/wadackel/ravelact/pull/4))
- *(readme)* clarify usage section headings ([#3](https://github.com/wadackel/ravelact/pull/3))
