#!/usr/bin/env sh
set -eu

origin="${NEOGATE_PUBLIC_ORIGIN:-${PUBLIC_BASE_URL:-http://localhost:8080}}"
origin="${origin%/}"

sed \
  -e "s#__NEOGATE_DEFAULT_BASE_URL__#${origin}/v1#g" \
  -e "s#__NEOGATE_INSTALL_ORIGIN__#${origin}#g" \
  /usr/share/nginx/html/install.template > /usr/share/nginx/html/install

chmod 0644 /usr/share/nginx/html/install
