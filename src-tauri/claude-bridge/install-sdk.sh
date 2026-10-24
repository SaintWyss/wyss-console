#!/bin/sh
# Builds the Claude Agent SDK virtual environment the Wyss authority verifies and runs.
#
#   claude-bridge/install-sdk.sh <new-directory>
#
# The environment is made with the standard library's `venv` (no activation scripts or `.pth`
# files that no package vouches for) and filled from `requirements.lock` with
# `--require-hashes --no-deps --only-binary :all:`: exactly the pinned wheels, nothing resolved
# at install time. It writes only inside <new-directory>, installs nothing globally and touches
# no Claude or Codex configuration. The authority later checks every file against pip's RECORD
# files and the bundled CLI against its pinned SHA-256 (`adapters::claude::pins`).
set -eu
[ "$#" -eq 1 ] || { echo "usage: $0 <new-directory>" >&2; exit 2; }
target=$1
here=$(cd "$(dirname "$0")" && pwd)
[ ! -e "$target" ] || { echo "refusing to reuse an existing path: $target" >&2; exit 2; }
/usr/bin/python3 -m venv --without-pip "$target"
# `ensurepip` is avoided so the tree holds only hashed wheels; pip itself runs from the system.
/usr/bin/python3 -m pip --python "$target/bin/python" install \
    --require-hashes --no-deps --only-binary :all: --no-cache-dir --disable-pip-version-check \
    -r "$here/requirements.lock"
chmod -R go-w "$target"
echo "built $target"
