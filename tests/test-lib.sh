#!/bin/sh
# Simplified test library for Gust tests.
# Modelled on git/t/test-lib.sh but stripped to what our tests need.

# Locate grit binary: prefer a local build, else fall back to PATH.
if test -z "$GUST_BIN"
then
	for candidate in \
		"$(dirname "$(dirname "$0")")/target/debug/grit" \
		"$(dirname "$(dirname "$0")")/target/release/grit"
	do
		if test -x "$candidate"
		then
			GUST_BIN="$candidate"
			break
		fi
	done
	if test -z "$GUST_BIN"
	then
		for f in /var/folders/*/T/cursor-sandbox-cache/*/cargo-target/debug/grit \
		          /tmp/cargo-target/debug/grit
		do
			if test -x "$f"
			then
				GUST_BIN="$f"
				break
			fi
		done
	fi
fi

if test -z "$GUST_BIN"
then
	echo "FATAL: could not locate grit binary (set GUST_BIN)" >&2
	exit 1
fi

GUST_BIN="$(cd "$(dirname "$GUST_BIN")" && pwd)/$(basename "$GUST_BIN")"

TEST_DIRECTORY="$(cd "$(dirname "$0")" && pwd)"
TRASH_DIRECTORY="${TRASH_DIRECTORY:-$TEST_DIRECTORY/trash}"
TEST_RESULTS_DIR="${TEST_DIRECTORY}/test-results"

test_count=0
test_pass=0
test_fail=0
test_skip=0
test_failures=""

if test -t 1 && command -v tput >/dev/null 2>&1
then
	RED="$(tput setaf 1)" GREEN="$(tput setaf 2)" YELLOW="$(tput setaf 3)" RESET="$(tput sgr0)"
else
	RED='' GREEN='' YELLOW='' RESET=''
fi

setup_trash () {
	rm -rf "$TRASH_DIRECTORY"
	mkdir -p "$TRASH_DIRECTORY"
	TEST_BIN_DIR="${TEST_DIRECTORY}/trash-bin"
	rm -rf "$TEST_BIN_DIR"
	mkdir -p "$TEST_BIN_DIR"
	cat >"$TEST_BIN_DIR/git" <<EOF
#!/bin/sh
exec "$GUST_BIN" "\$@"
EOF
	chmod +x "$TEST_BIN_DIR/git"
	cat >"$TEST_BIN_DIR/grit" <<EOF
#!/bin/sh
exec "$GUST_BIN" "\$@"
EOF
	chmod +x "$TEST_BIN_DIR/grit"
	export PATH="$TEST_BIN_DIR:$PATH"
	cd "$TRASH_DIRECTORY" || exit 1
	"$GUST_BIN" init -q || exit 1
}

setup_trash

HOME="$TRASH_DIRECTORY"
export HOME

if test -z "$TEST_VERBOSE"
then
	GIT_QUIET=-q
else
	GIT_QUIET=
fi

ZERO_OID=0000000000000000000000000000000000000000
SQ="'"
LF='
'
export ZERO_OID SQ LF

if test -n "$GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME"
then
	git config --global init.defaultBranch "$GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME"
fi

# ── helpers ──

test_grep () {
	local invert=""
	while test $# -gt 0; do
		case "$1" in
		-e) shift; break ;;
		-v|!) invert="-v"; shift ;;
		--) shift; break ;;
		-*) shift ;;
		*) break ;;
		esac
	done
	local pattern="$1"
	shift
	grep $invert "$pattern" "$@"
}

test_create_repo () {
	local repo="$1"
	mkdir -p "$repo" &&
	(
		cd "$repo" &&
		git init &&
		git config user.name "Test User" &&
		git config user.email "test@example.com"
	)
}

test_write_lines () {
	while test $# -gt 0; do
		printf '%s\n' "$1"
		shift
	done
}

test_config () {
	local key="$1" val="$2"
	git config "$key" "$val"
}

test_file_not_empty () {
	if ! test -s "$1"
	then
		echo >&2 "test_file_not_empty: '$1' is empty"
		return 1
	fi
}

test_might_fail () {
	"$@"
	return 0
}

test_set_prereq () {
	:
}

test_path_is_file () { test -f "$1"; }
test_path_is_dir  () { test -d "$1"; }
test_path_is_missing () { ! test -e "$1"; }

test_line_count () {
	local op="$1" count="$2" file="$3"
	local actual
	actual=$(wc -l <"$file")
	actual=$(echo "$actual" | tr -d ' ')
	if test "$actual" "$op" "$count"
	then
		return 0
	else
		echo >&2 "test_line_count: expected $count lines ($op), got $actual in '$file'"
		return 1
	fi
}

test_must_be_empty () {
	if test -s "$1"
	then
		echo >&2 "test_must_be_empty: file '$1' is not empty"
		return 1
	fi
	return 0
}

test_have_prereq () {
	case "$1" in
	POSIXPERM) return 0 ;;
	SYMLINKS)  return 0 ;;
	*)         return 1 ;;
	esac
}

test_tick () {
	if test -z "${test_tick+set}"
	then
		test_tick=1112911993
	else
		test_tick=$(($test_tick + 60))
	fi
	GIT_COMMITTER_DATE="$test_tick -0700"
	GIT_AUTHOR_DATE="$test_tick -0700"
	export GIT_COMMITTER_DATE GIT_AUTHOR_DATE
}

test_commit () {
	local file="$1.t"
	printf '%s' "${2-$1}" >"$file"
	git add "$file"
	test_tick
	git commit -q -m "$1"
	git tag "$1"
}

test_cmp () {
	diff -u "$1" "$2"
}

# ── core test functions ──

test_expect_success () {
	local description="$1"
	local commands="$2"
	test_count=$(($test_count + 1))
	(
		set -e
		cd "$TRASH_DIRECTORY" || exit 1
		eval "$commands"
	)
	local result=$?
	if test $result -eq 0
	then
		test_pass=$(($test_pass + 1))
		if test -n "$TEST_VERBOSE"
		then
			printf '%sok %d - %s%s\n' "$GREEN" "$test_count" "$description" "$RESET"
		else
			printf '.'
		fi
	else
		test_fail=$(($test_fail + 1))
		test_failures="$test_failures
  FAIL $test_count: $description"
		printf '%snot ok %d - %s%s\n' "$RED" "$test_count" "$description" "$RESET" >&2
	fi
}

test_expect_failure () {
	local description="$1"
	local commands="$2"
	test_count=$(($test_count + 1))
	(
		set -e
		cd "$TRASH_DIRECTORY" || exit 1
		eval "$commands"
	)
	local result=$?
	if test $result -ne 0
	then
		test_pass=$(($test_pass + 1))
		test_skip=$(($test_skip + 1))
		printf '%sok %d - %s # TODO expected failure%s\n' "$YELLOW" "$test_count" "$description" "$RESET"
	else
		test_fail=$(($test_fail + 1))
		test_failures="$test_failures
  FAIL(expected) $test_count: $description"
		printf '%snot ok %d - %s%s\n' "$RED" "$test_count" "$description" "$RESET" >&2
	fi
}

test_when_finished () {
	_twf_cmd="${_twf_cmd:+${_twf_cmd}; }$*"
	trap 'eval "$_twf_cmd"' EXIT
}

test_must_fail () {
	set +e
	"$@"
	status=$?
	set -e
	test $status -ne 0
}

test_expect_code () {
	local expected_code="$1"
	shift
	set +e
	"$@"
	local actual_code=$?
	set -e
	if test "$actual_code" = "$expected_code"
	then
		return 0
	else
		echo >&2 "test_expect_code: expected exit code $expected_code, got $actual_code from: $*"
		return 1
	fi
}

test_done () {
	printf '\n'
	echo "# Tests: $test_count  Pass: $test_pass  Fail: $test_fail  Skip: $test_skip"
	if test $test_fail -gt 0
	then
		echo "${RED}FAILED:${RESET}$test_failures" >&2
		exit 1
	fi
	exit 0
}
