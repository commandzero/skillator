#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
source scripts/tools-versions.sh
mkdir -p .tools/bin
case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) platform=darwin_arm64; digest=a21ba7366d8329e7223faee0ed69eb13da27fe8acabb356bb7eb0b7f1e1cb6d8 ;;
  Darwin-x86_64) platform=darwin_amd64; digest=17ffc17fed8f0258ef6ad4aed932d3272464c7ef7d64e1cb0d65aa97c9752107 ;;
  Linux-x86_64) platform=linux_amd64; digest=900919a84f2229bac68ca9cd4103ea297abc35e9689ebb842c6e34a3d1b01b0a ;;
  Linux-aarch64) platform=linux_arm64; digest=21bc0dfb57a913fe175298c2a9e906ee630f747cb66d0a934d0d4b69f4ee1235 ;;
  *) echo 'Unsupported validation host' >&2; exit 1 ;;
esac
archive="actionlint_${ACTIONLINT_VERSION}_${platform}.tar.gz"
curl --fail --location --retry 3 "https://github.com/rhysd/actionlint/releases/download/v${ACTIONLINT_VERSION}/${archive}" -o ".tools/${archive}"
printf '%s  %s\n' "$digest" ".tools/${archive}" | shasum -a 256 --check
tar -xzf ".tools/${archive}" -C .tools/bin actionlint
npm install --prefix .tools --no-audit --no-fund --save-exact "@fission-ai/openspec@${OPENSPEC_VERSION}"
rustup run 1.97.1 cargo install okf --version "$OKF_VERSION" --locked --root .tools
echo 'Tools installed under .tools. ShellCheck must also be on PATH.'
