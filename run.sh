#!/usr/bin/env bash
# Compila (si hace falta) y abre Microverse.
set -euo pipefail
cd "$(dirname "$0")"

if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck source=/dev/null
  source "$HOME/.cargo/env"
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "Aún no está Rust. Abre una terminal y pega esto:"
  echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y"
  echo "  source \"\$HOME/.cargo/env\""
  echo "Luego vuelve a ejecutar:  ./run.sh"
  exit 1
fi

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target-linux}"
echo "Compilando Microverse (release)…"
exec cargo run --release
