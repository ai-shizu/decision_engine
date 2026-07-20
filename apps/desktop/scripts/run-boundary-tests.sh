#!/usr/bin/env bash

set -u -o pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
OUT_DIR="$PROJECT_DIR/.boundary-tests-out"

resolve_python() {
  if [[ -n "${PKB_PYTHON:-}" && -x "${PKB_PYTHON}" ]]; then
    printf '%s\n' "$PKB_PYTHON"
    return 0
  fi
  for candidate in python3.13 python3.12 python3.11 python3; do
    if command -v "$candidate" >/dev/null 2>&1; then
      command -v "$candidate"
      return 0
    fi
  done
  return 1
}

cleanup() {
  if [[ "$OUT_DIR" == "$PROJECT_DIR/.boundary-tests-out" ]]; then
    rm -rf -- "$OUT_DIR"
  fi
}

trap cleanup EXIT
cleanup
mkdir -p "$OUT_DIR"
cd "$PROJECT_DIR" || exit 1

PYTHON_EXE="$(resolve_python)" || {
  printf '%s\n' "Python not found. Set PKB_PYTHON to Python 3.11 or newer." >&2
  exit 1
}
BOUNDARY_IDENTITY_KEY="${PKB_IDENTITY_ROOT_KEY_HEX:-5151515151515151515151515151515151515151515151515151515151515151}"

printf '%s\n' "-- generating golden fixtures --"
for generator in tests-runtime/gen_*_fixture.py; do
  base="$(basename "$generator" .py)"
  case "$base" in
    gen_manifest_fixture) output_name="golden.json" ;;
    gen_tensor_report_fixture) output_name="tensor_golden.json" ;;
    *) output_name="${base#gen_}"; output_name="${output_name%_fixture}.json" ;;
  esac
  if ! PKB_IDENTITY_ROOT_KEY_HEX="$BOUNDARY_IDENTITY_KEY" \
    "$PYTHON_EXE" "$generator" "$OUT_DIR/$output_name"; then
    printf '%s\n' "$generator failed" >&2
    exit 1
  fi
done

printf '%s\n' "-- compiling boundary suite (tsconfig.boundary.json) --"
if ! node ./node_modules/typescript/bin/tsc -p tsconfig.boundary.json; then
  printf '%s\n' "tsc -p tsconfig.boundary.json failed" >&2
  exit 1
fi

printf '%s\n' '{"type":"commonjs"}' > "$OUT_DIR/package.json"

shopt -s nullglob
suites=("$OUT_DIR"/tests-runtime/*.test.js)
if (( ${#suites[@]} == 0 )); then
  printf '%s\n' "no boundary test suites compiled" >&2
  exit 1
fi

overall_failed=0
for suite in "${suites[@]}"; do
  printf '%s\n' "-- running $(basename "$suite") --"
  if ! node "$suite"; then
    overall_failed=1
  fi
done

printf 'BOUNDARY_EXIT=%d\n' "$overall_failed"
exit "$overall_failed"
