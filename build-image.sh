#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
version="${1:-${RELEASE_VERSION:-0.1.0-alpha.1}}"
image_repository="${IMAGE_REPOSITORY:-guowenju/nut-web-manager}"
revision="${GITHUB_SHA:-$(git -C "$repo_root" rev-parse HEAD 2>/dev/null || printf unknown)}"
image="${image_repository}:${version}"

if [[ ! "$version" =~ ^[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}$ ]]; then
    printf 'Invalid image version: %q\n' "$version" >&2
    exit 2
fi

printf 'Building Docker image %s\n' "$image"
docker build \
    --file "$repo_root/Dockerfile" \
    --build-arg "VERSION=$version" \
    --build-arg "REVISION=$revision" \
    --tag "$image" \
    "$repo_root"

if [[ "${TAG_LATEST:-false}" == "true" ]]; then
    docker tag "$image" "${image_repository}:latest"
    printf 'Tagged %s\n' "${image_repository}:latest"
fi

printf 'Built %s\n' "$image"
