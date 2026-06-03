#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'USAGE'
Usage:
  update_sqlx_migration_checksum.sh [migration.sql ...]

Updates _sqlx_migrations.checksum for already-applied SQLx migrations.
With no arguments, updates every .sql file in ./migrations.

Environment:
  DATABASE_URL  Optional PostgreSQL connection URL. If unset, loaded from .env.
  PSQL_BIN      Optional psql binary name/path. Defaults to psql.

Examples:
  cd backend
  scripts/update_sqlx_migration_checksum.sh

  backend/scripts/update_sqlx_migration_checksum.sh backend/migrations/0002_billing_outbox.sql

  cd backend
  scripts/update_sqlx_migration_checksum.sh 0002_billing_outbox.sql
USAGE
}

die() {
    echo "error: $*" >&2
    exit 1
}

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
backend_dir="$(cd "$script_dir/.." && pwd -P)"
repo_dir="$(cd "$backend_dir/.." && pwd -P)"
loaded_env_file=""

load_database_url_from_env_file() {
    local env_file line value

    [[ -z "${DATABASE_URL:-}" ]] || return 0

    for env_file in "$PWD/.env" "$backend_dir/.env" "$repo_dir/.env"; do
        [[ -f "$env_file" ]] || continue

        while IFS= read -r line || [[ -n "$line" ]]; do
            [[ "$line" =~ ^[[:space:]]*(#|$) ]] && continue
            [[ "$line" =~ ^[[:space:]]*(export[[:space:]]+)?DATABASE_URL[[:space:]]*=(.*)$ ]] || continue

            value=${BASH_REMATCH[2]}
            value=${value%$'\r'}

            if [[ "$value" =~ ^[[:space:]]*\"(.*)\"[[:space:]]*$ ]]; then
                value=${BASH_REMATCH[1]}
            elif [[ "$value" =~ ^[[:space:]]*\'(.*)\'[[:space:]]*$ ]]; then
                value=${BASH_REMATCH[1]}
            else
                value=${value%%[[:space:]]#*}
                while [[ "$value" =~ ^[[:space:]] ]]; do value=${value#?}; done
                while [[ "$value" =~ [[:space:]]$ ]]; do value=${value%?}; done
            fi

            [[ -n "$value" ]] || die "DATABASE_URL in $env_file is empty"
            export DATABASE_URL=$value
            loaded_env_file=$env_file
            return
        done < "$env_file"
    done
}

resolve_sql_file() {
    local input=$1
    local candidate

    if [[ "$input" = /* ]]; then
        [[ -f "$input" ]] && {
            echo "$input"
            return
        }
        die "SQL file not found: $input"
    fi

    for candidate in \
        "$PWD/$input" \
        "$backend_dir/$input" \
        "$backend_dir/migrations/$input"
    do
        if [[ -f "$candidate" ]]; then
            (cd "$(dirname "$candidate")" && printf '%s/%s\n' "$(pwd -P)" "$(basename "$candidate")")
            return
        fi
    done

    die "SQL file not found: $input"
}

update_migration_checksum() {
    local sql_file base_name version checksum_hex updated_version

    sql_file=$(resolve_sql_file "$1")
    base_name=$(basename "$sql_file")

    if [[ ! "$base_name" =~ ^0*([0-9]+)_.+\.sql$ ]]; then
        die "migration filename must look like 0002_description.sql: $base_name"
    fi

    version=$((10#${BASH_REMATCH[1]}))
    checksum_hex=$(shasum -a 384 "$sql_file" | awk '{print $1}')

    updated_version=$(
        "$PSQL_BIN" "$DATABASE_URL" \
            -v ON_ERROR_STOP=1 \
            -t -A \
            -c "UPDATE _sqlx_migrations SET checksum = decode('$checksum_hex', 'hex') WHERE version = $version RETURNING version;"
    )

    if [[ -z "$updated_version" ]]; then
        die "migration version $version is not present in _sqlx_migrations"
    fi

    echo "updated _sqlx_migrations"
    echo "  file:     $sql_file"
    echo "  version:  $version"
    echo "  checksum: $checksum_hex"
}

if [[ ${1:-} = "-h" || ${1:-} = "--help" ]]; then
    usage
    exit 0
fi

load_database_url_from_env_file
[[ -n "${DATABASE_URL:-}" ]] || die "DATABASE_URL is not set and no .env with DATABASE_URL was found"
command -v shasum >/dev/null 2>&1 || die "shasum is required"

PSQL_BIN=${PSQL_BIN:-psql}
command -v "$PSQL_BIN" >/dev/null 2>&1 || die "psql not found: $PSQL_BIN"

if [[ -n "$loaded_env_file" ]]; then
    echo "loaded DATABASE_URL from $loaded_env_file"
fi

if [[ $# -eq 0 ]]; then
    migration_files=("$backend_dir"/migrations/*.sql)
    [[ -e "${migration_files[0]}" ]] || die "no migration SQL files found in $backend_dir/migrations"

    for sql_file in "${migration_files[@]}"; do
        update_migration_checksum "$sql_file"
    done
else
    for sql_file in "$@"; do
        update_migration_checksum "$sql_file"
    done
fi
