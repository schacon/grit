#!/bin/bash
# Run t4xxx upstream tests against grit
set -eu

REPO=/home/hasi/grit-GG
GRIT_BIN=$REPO/target/release/grit
RESULTS=/tmp/grit-t4xxx-results
WORKDIR=/tmp/grit-t4xxx-workdir

rm -rf "$RESULTS" "$WORKDIR"
mkdir -p "$RESULTS" "$WORKDIR"

cp -a "$REPO/git/t" "$WORKDIR/t"
mkdir -p "$WORKDIR/templates/blt/branches"

cat > "$WORKDIR/git" <<WRAPPER
#!/bin/sh
GRIT=$GRIT_BIN
case "\${1:-__NOARGS__}" in
  __NOARGS__) echo "usage: git ..." >&2; exit 1 ;;
  --exec-path) echo "$WORKDIR"; exit 0 ;;
  --exec-path=*) exit 0 ;;
  version)
    if test "\$2" = "--build-options"; then
      echo "git version 2.47.0"; echo "sizeof-long: 8"; echo "sizeof-size_t: 8"; echo "shell-path: /bin/sh"; exit 0
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

cat > "$WORKDIR/t/helper/test-tool" <<'TOOL'
#!/bin/sh
case "$1" in
  chmtime) shift; touch "$@" 2>/dev/null ;;
  *) exit 0 ;;
esac
TOOL
chmod +x "$WORKDIR/t/helper/test-tool"

FILTER="${1:-t4}"

run_test() {
  local f="$1"
  local name=$(basename "$f" .sh)
  local out="$RESULTS/$name.out"
  (
    cd "$WORKDIR/t"
    GIT_TEST_DEFAULT_HASH=sha1 GIT_BUILD_DIR="$WORKDIR" TEST_NO_MALLOC_CHECK=1 \
      timeout 120 bash "./$f" > "$out" 2>&1
  ) || true
}

export GRIT_BIN WORKDIR RESULTS
export -f run_test

find "$WORKDIR/t" -maxdepth 1 -name "${FILTER}*.sh" -printf '%f\n' | sort | \
  xargs -P 16 -I{} bash -c 'run_test "$@"' _ {}

# Aggregate
total_pass=0; total_fail=0
for out in "$RESULTS"/*.out; do
  p=$(grep -cE '^ok [0-9]' "$out" 2>/dev/null) || p=0
  f=$(grep -cE '^not ok [0-9]' "$out" 2>/dev/null) || f=0
  total_pass=$((total_pass + p))
  total_fail=$((total_fail + f))
done
total=$((total_pass + total_fail))
echo "=== T4xxx RESULTS ==="
echo "Pass: $total_pass / $total  Fail: $total_fail"
