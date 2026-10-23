#!/usr/bin/env bash
# Publish the docs site to its server (https://jengine.hirakame.com).
#
#   ./deploy.sh               build, then upload
#   ./deploy.sh --no-build    upload the dist/ already built
#   ./deploy.sh --no-rustdoc  skip `cargo doc` (an existing target/doc still goes up)
#
# Where it goes comes from the environment - no address is kept in this file:
#
#   JENGINE_DOCS_HOST   the server's address; several, space-separated, are
#                       tried in order and the first that answers ssh is used
#                       (a LAN address, then a public one)
#   JENGINE_DOCS_USER   the ssh user on it
#   JENGINE_DOCS_DIR    the site's folder there, e.g. /home/<user>/jengine-docs
#   JENGINE_DOCS_PORT   the port the container serves on (default 8091)
#
# On the server the site is an nginx:alpine container, `jengine-docs`, serving
# $JENGINE_DOCS_DIR/html - a relative symlink into releases/, so a deploy is
# swapped in whole and the last few are kept to point back at. A reverse proxy
# in front of it terminates TLS for the domain.
# Needs ssh and tar only (Git Bash on Windows has both; no rsync).
set -euo pipefail

missing=()
[ -n "${JENGINE_DOCS_HOST:-}" ] || missing+=(JENGINE_DOCS_HOST)
[ -n "${JENGINE_DOCS_USER:-}" ] || missing+=(JENGINE_DOCS_USER)
[ -n "${JENGINE_DOCS_DIR:-}" ] || missing+=(JENGINE_DOCS_DIR)
if [ "${#missing[@]}" -gt 0 ]; then
  echo "deploy.sh: set ${missing[*]} first - the server's address(es), the ssh user and the site's folder on it." >&2
  echo "  e.g. JENGINE_DOCS_HOST=docs.example.org JENGINE_DOCS_USER=me JENGINE_DOCS_DIR=/home/me/jengine-docs ./deploy.sh" >&2
  exit 2
fi

HOST=
for candidate in $JENGINE_DOCS_HOST; do
  if ssh -o BatchMode=yes -o ConnectTimeout=4 "$JENGINE_DOCS_USER@$candidate" true 2>/dev/null; then
    HOST="$JENGINE_DOCS_USER@$candidate"
    break
  fi
done
if [ -z "$HOST" ]; then
  echo "deploy.sh: no host in JENGINE_DOCS_HOST ($JENGINE_DOCS_HOST) answered ssh as $JENGINE_DOCS_USER" >&2
  exit 1
fi
DIR="$JENGINE_DOCS_DIR"
PORT="${JENGINE_DOCS_PORT:-8091}"
KEEP=5

BUILD=1
RUSTDOC=1
for arg in "$@"; do
  case "$arg" in
    --no-build) BUILD=0 ;;
    --no-rustdoc) RUSTDOC=0 ;;
    *) echo "unknown option: $arg" >&2; exit 2 ;;
  esac
done

cd "$(dirname "$0")"
[ "$BUILD" = 0 ] || npm run build
[ -f dist/index.html ] || { echo "dist/index.html missing - build first" >&2; exit 1; }

# The Rust API reference the sidebar links to (/rustdoc/jengine_core/): core's
# own docs only, since its dependencies are on docs.rs. About 85 MB.
if [ "$RUSTDOC" = 1 ]; then
  (cd .. && cargo doc -p jengine-core --no-deps -q)
fi
if [ -f ../target/doc/jengine_core/index.html ]; then
  rm -rf dist/rustdoc
  cp -r ../target/doc dist/rustdoc
else
  echo "warning: no rustdoc in ../target/doc - /rustdoc/ will be missing" >&2
fi

STAMP=$(date +%Y%m%d-%H%M%S)
echo "uploading release $STAMP to $HOST"
tar -C dist -czf - . | ssh "$HOST" "set -e
  mkdir -p $DIR/releases/$STAMP $DIR/conf
  tar -xzf - -C $DIR/releases/$STAMP
  # every directory and file readable by the container's nginx
  chmod -R a+rX $DIR/releases/$STAMP"
# cat > keeps the inode, so the container's single-file mount sees the change
ssh "$HOST" "cat > $DIR/conf/default.conf" < nginx.conf

ssh "$HOST" "set -e
  cd $DIR
  ln -sfn releases/$STAMP html.new && mv -T html.new html
  if ! docker ps -a --format '{{.Names}}' | grep -qx jengine-docs; then
    docker run -d --name jengine-docs --restart unless-stopped \
      -p $PORT:80 \
      -v $DIR:/usr/share/nginx:ro \
      -v $DIR/conf/default.conf:/etc/nginx/conf.d/default.conf:ro \
      nginx:alpine >/dev/null
    echo 'started jengine-docs'
  else
    docker start jengine-docs >/dev/null
    docker exec jengine-docs nginx -t -q && docker exec jengine-docs nginx -s reload
  fi
  ls -1dt releases/* | tail -n +$((KEEP + 1)) | xargs -r rm -rf
  curl -fsS -o /dev/null -w 'local check: %{http_code}\n' http://127.0.0.1:$PORT/"
echo "live at https://jengine.hirakame.com/"
