#!/bin/bash
# Run all upstream git/t/ tests against grit in an isolated temp directory.
# Results go to /tmp/grit-upstream-results/*.out
set -eu

REPO="$(cd "$(dirname "$0")/.." && pwd)"
GRIT_BIN=$REPO/target/release/grit
RESULTS=/tmp/grit-upstream-results
WORKDIR=/tmp/grit-upstream-workdir
TEST_FILTER="${1:-}"

rm -rf "$RESULTS" "$WORKDIR"
mkdir -p "$RESULTS" "$WORKDIR"

# Copy the upstream test infrastructure to an isolated location
cp -a "$REPO/git/t" "$WORKDIR/t"
mkdir -p "$WORKDIR/templates/blt/branches"

# Create the git wrapper in the workdir (not in the repo!)
mkdir -p "$WORKDIR/t/helper"
cat > "$WORKDIR/git" <<WRAPPER
#!/bin/sh
GRIT=$GRIT_BIN
case "\${1:-__NOARGS__}" in
  __NOARGS__) echo "usage: git ..." >&2; exit 1 ;;
  --exec-path) echo "$WORKDIR"; exit 0 ;;
  --exec-path=*) exec "\$GRIT" "\$@" ;;
  version)
    if test "\$2" = "--build-options"; then
      echo "git version 2.47.0"; echo "sizeof-long: 8"; echo "sizeof-size_t: 8"; echo "shell-path: /bin/sh"; echo "default-hash: sha1"; exit 0
    fi
    echo "git version 2.47.0"; exit 0 ;;
  --version) echo "git version 2.47.0"; exit 0 ;;
  --html-path) echo "/usr/share/doc/git"; exit 0 ;;
  --man-path) echo "/usr/share/man"; exit 0 ;;
  --info-path) echo "/usr/share/info"; exit 0 ;;
esac
exec "\$GRIT" "\$@"
WRAPPER
chmod +x "$WORKDIR/git"

# Create GIT-BUILD-OPTIONS
cat > "$WORKDIR/GIT-BUILD-OPTIONS" <<EOF
SHELL_PATH='/bin/sh'
PERL_PATH='/usr/bin/perl'
DIFF='diff'
GIT_TEST_CMP='diff -u'
NO_PERL=
NO_PYTHON=YesPlease
NO_UNIX_SOCKETS=
PAGER_ENV='LESS=FRX LV=-c'
DC_SHA1=
SANITIZE_LEAK=
SANITIZE_ADDRESS=
X=''
GIT_TEST_TEMPLATE_DIR='$WORKDIR/templates/blt'
GIT_TEST_GITPERLLIB=''
GIT_TEST_CMP_USE_COPIED_CONTEXT=
GIT_TEST_INDEX_VERSION=4
EOF

# Fake test-tool
cat > "$WORKDIR/t/helper/test-tool" <<TOOL
#!/bin/sh
case "\$1" in
  trace2)
    exec "$GRIT_BIN" test-tool "\$@"
    ;;
  revision-walking)
    exec "$GRIT_BIN" test-tool "\$@"
    ;;
  chmtime) shift; exec "$GRIT_BIN" test-tool chmtime "\$@" ;;
  *)
    exec "$GRIT_BIN" test-tool "\$@"
    ;;
esac
TOOL
chmod +x "$WORKDIR/t/helper/test-tool"

echo "Running upstream tests from $WORKDIR/t against $GRIT_BIN"
echo "Results: $RESULTS"

run_test() {
  local f="$1"
  local name=$(basename "$f" .sh)
  local out="$RESULTS/$name.out"
  local runner=()
  if command -v timeout >/dev/null 2>&1; then
    runner=(timeout 60)
  fi
  (
    cd "$WORKDIR/t"
    GIT_BUILD_DIR="$WORKDIR" TEST_NO_MALLOC_CHECK=1 TAR="${TAR:-tar}" \
      "${runner[@]}" bash "./$f" > "$out" 2>&1
  ) || true
}

export GRIT_BIN WORKDIR RESULTS
export -f run_test

if test -n "$TEST_FILTER"
then
  PATTERN="${TEST_FILTER}*.sh"
else
  PATTERN='t[0-9]*.sh'
fi

find "$WORKDIR/t" -maxdepth 1 -name "$PATTERN" -exec basename {} \; | sort | \
  xargs -P 16 -I{} bash -c 'run_test "$@"' _ {}

# Aggregate
bash "$REPO/scripts/aggregate-upstream.sh"
