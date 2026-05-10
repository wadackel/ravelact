#!/usr/bin/env bash
set -euo pipefail

readonly DEFAULT_RELEASE_BASE_URL="https://github.com/wadackel/ravelact/releases"

error() {
  printf '::error::%s\n' "$*" >&2
}

usage() {
  cat <<'USAGE'
Usage: install-ravelact-action.sh [--resolve-only]

Installs ravelact from GitHub Releases for the current GitHub Actions runner.
USAGE
}

resolve_asset() {
  local runner_os="${RUNNER_OS:-}"
  local runner_arch="${RUNNER_ARCH:-}"

  case "${runner_os}:${runner_arch}" in
    Linux:X64)
      printf 'ravelact-linux-amd64'
      ;;
    Linux:ARM64)
      printf 'ravelact-linux-arm64'
      ;;
    macOS:X64)
      printf 'ravelact-darwin-amd64'
      ;;
    macOS:ARM64)
      printf 'ravelact-darwin-arm64'
      ;;
    *)
      error "unsupported runner platform: RUNNER_OS=${runner_os:-<unset>} RUNNER_ARCH=${runner_arch:-<unset>}"
      return 1
      ;;
  esac
}

resolve_version() {
  local version="${RAVELACT_VERSION:-}"
  local action_ref="${RAVELACT_ACTION_REF:-}"

  if [ -n "$version" ]; then
    printf '%s' "$version"
    return 0
  fi

  case "$action_ref" in
    v*)
      printf '%s' "$action_ref"
      ;;
    *)
      error "version input is required when the action ref is not a v* release tag; set version: latest to opt in to the latest GitHub Release"
      return 1
      ;;
  esac
}

release_url() {
  local version="$1"
  local asset="$2"
  local release_base_url="${RAVELACT_RELEASE_BASE_URL:-$DEFAULT_RELEASE_BASE_URL}"

  release_base_url="${release_base_url%/}"

  if [ "$version" = "latest" ]; then
    printf '%s/latest/download/%s' "$release_base_url" "$asset"
  else
    printf '%s/download/%s/%s' "$release_base_url" "$version" "$asset"
  fi
}

download() {
  local url="$1"
  local output="$2"
  local label="$3"

  if ! curl --fail --show-error --location --retry 3 --retry-delay 2 --output "$output" "$url"; then
    error "failed to download ${label}: ${url}"
    return 1
  fi
}

verify_checksum() {
  local checksum_file="$1"
  local asset="$2"
  local binary_path="$3"
  local expected_sha

  expected_sha="$(awk -v asset="$asset" '$2 == asset { print $1; found = 1 } END { if (!found) exit 1 }' "$checksum_file")" || {
    error "checksums.txt does not contain an entry for ${asset}"
    return 1
  }

  if ! printf '%s  %s\n' "$expected_sha" "$binary_path" | shasum -a 256 -c - >/dev/null; then
    error "checksum verification failed for ${asset}"
    return 1
  fi
}

main() {
  local resolve_only=false

  case "${1:-}" in
    '')
      ;;
    --resolve-only)
      resolve_only=true
      ;;
    -h|--help)
      usage
      return 0
      ;;
    *)
      error "unknown argument: $1"
      usage >&2
      return 2
      ;;
  esac

  local version
  local asset
  local asset_url
  local checksums_url
  version="$(resolve_version)"
  asset="$(resolve_asset)"
  asset_url="$(release_url "$version" "$asset")"
  checksums_url="$(release_url "$version" "checksums.txt")"

  if [ "$resolve_only" = true ]; then
    printf 'version=%s\n' "$version"
    printf 'asset=%s\n' "$asset"
    printf 'asset_url=%s\n' "$asset_url"
    printf 'checksums_url=%s\n' "$checksums_url"
    return 0
  fi

  if [ -z "${RUNNER_TEMP:-}" ]; then
    error "RUNNER_TEMP is required"
    return 1
  fi
  if [ -z "${GITHUB_PATH:-}" ]; then
    error "GITHUB_PATH is required"
    return 1
  fi

  local work_dir="${RUNNER_TEMP}/ravelact-install"
  local install_dir="${RUNNER_TEMP}/ravelact/bin"
  local binary_tmp="${work_dir}/${asset}"
  local checksum_file="${work_dir}/checksums.txt"

  rm -rf "$work_dir"
  mkdir -p "$work_dir" "$install_dir"

  download "$asset_url" "$binary_tmp" "$asset"
  download "$checksums_url" "$checksum_file" "checksums.txt"
  verify_checksum "$checksum_file" "$asset" "$binary_tmp"

  mv "$binary_tmp" "${install_dir}/ravelact"
  chmod 0755 "${install_dir}/ravelact"
  if ! "${install_dir}/ravelact" --version >/dev/null 2>&1; then
    error "installed ${asset} from ${asset_url} is not executable on this runner"
    return 1
  fi
  printf '%s\n' "$install_dir" >> "$GITHUB_PATH"
  printf 'Installed ravelact %s to %s\n' "$version" "${install_dir}/ravelact"
}

main "$@"
