# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.9](https://github.com/wadackel/ravelact/compare/v0.0.8...v0.0.9) - 2026-05-31

### Added

- add ravelact logo to browse and README branding ([#56](https://github.com/wadackel/ravelact/pull/56))

### Other

- drop PoC framing for browse in README and CLI help ([#59](https://github.com/wadackel/ravelact/pull/59))
- *(deps)* pin dependencies ([#55](https://github.com/wadackel/ravelact/pull/55))

## [0.0.8](https://github.com/wadackel/ravelact/compare/v0.0.7...v0.0.8) - 2026-05-25

### Added

- *(browse)* migrate to Protocol Buffers + ConnectRPC ([#45](https://github.com/wadackel/ravelact/pull/45))
- *(browse,web)* surface `if:` conditions in Details tab ([#42](https://github.com/wadackel/ravelact/pull/42))
- *(browse,web)* show file path relative to browse root and add Copy button ([#41](https://github.com/wadackel/ravelact/pull/41))
- *(browse)* add "Powered by ravelact" credit overlay with build-time version ([#40](https://github.com/wadackel/ravelact/pull/40))
- *(ci)* enforce per-file ≥90% line coverage across src/ ([#34](https://github.com/wadackel/ravelact/pull/34))

### Fixed

- *(browse)* broaden remote-URL parser and support GitHub Enterprise ([#24](https://github.com/wadackel/ravelact/pull/24))

### Other

- *(browse)* raise mod.rs coverage to >=90% and drop subprocess soft floor ([#53](https://github.com/wadackel/ravelact/pull/53))
- *(readme)* add browse screenshots and full Browse section ([#37](https://github.com/wadackel/ravelact/pull/37))
- *(ci)* integrate zizmor into lint-actions ([#35](https://github.com/wadackel/ravelact/pull/35))

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
