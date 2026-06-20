#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ROOT_ENV="${ROOT_DIR}/.env"
BACKEND_ENV="${ROOT_DIR}/backend/.env"

load_env_file() {
  local env_file=$1

  [[ -z "${DATABASE_URL:-}" && -f "${env_file}" ]] || return 0

  set -a
  # shellcheck disable=SC1090
  source "${env_file}"
  set +a
}

if [[ -n "${NEOGATE_ENV_FILE:-}" ]]; then
  load_env_file "${NEOGATE_ENV_FILE}"
fi

load_env_file "${ROOT_ENV}"
load_env_file "${BACKEND_ENV}"

if [[ -z "${DATABASE_URL:-}" ]]; then
  echo "DATABASE_URL is required. Set it in the environment, NEOGATE_ENV_FILE, .env, or backend/.env." >&2
  exit 1
fi

if ! command -v psql >/dev/null 2>&1; then
  echo "psql is required but was not found in PATH." >&2
  exit 1
fi

psql "${DATABASE_URL}" -v ON_ERROR_STOP=1 <<'SQL'
DROP SCHEMA IF EXISTS public CASCADE;
CREATE SCHEMA public;
GRANT ALL ON SCHEMA public TO CURRENT_USER;
GRANT USAGE ON SCHEMA public TO PUBLIC;
SQL

declare -a env_files=("${ROOT_ENV}" "${BACKEND_ENV}")

if [[ -n "${NEOGATE_ENV_FILE:-}" ]]; then
  env_files+=("${NEOGATE_ENV_FILE}")
fi

for env_file in "${env_files[@]}"; do
  if [[ -f "${env_file}" ]]; then
    rm -f "${env_file}"
    echo "Removed ${env_file}"
  fi
done

echo "Database schema and first-run environment reset complete."
