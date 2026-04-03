#!/bin/sh
# Simplified test library for Gust tests.
# Modelled on git/t/test-lib.sh but stripped to what our tests need.
#
# Usage in test scripts:
#   . ./test-lib.sh
#   test_expect_success 'description' 'commands'
#   test_done

# Locate grit binary: prefer a local build, else fall back to PATH.
if test -z "$GUST_BIN"
then
	# Look in common cargo output locations
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
	# Also check the sandbox cache location
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

# Resolve GUST_BIN to an absolute path so wrapper scripts work regardless of cwd.
GUST_BIN="$(cd "$(dirname "$GUST_BIN")" && pwd)/$(basename "$GUST_BIN")"

# Test environment
TEST_DIRECTORY="$(cd "$(dirname "$0")" && pwd)"
TRASH_DIRECTORY="${TRASH_DIRECTORY:-$TEST_DIRECTORY/trash}"
TEST_RESULTS_DIR="${TEST_DIRECTORY}/test-results"

# Counters
test_count=0
test_pass=0
test_fail=0
test_skip=0
test_failures=""

# Colour
if test -t 1 && command -v tput >/dev/null 2>&1
then
	RED="$(tput setaf 1)" GREEN="$(tput setaf 2)" YELLOW="$(tput setaf 3)" RESET="$(tput sgr0)"
else
	RED='' GREEN='' YELLOW='' RESET=''
fi

# Set up a fresh trash directory for this test script.
setup_trash () {
	rm -rf "$TRASH_DIRECTORY"
	mkdir -p "$TRASH_DIRECTORY"
	mkdir -p "$TRASH_DIRECTORY/.bin"
	# Write a 'git' wrapper script that calls grit
	cat >"$TRASH_DIRECTORY/.bin/git" <<EOF
#!/bin/sh
exec "$GUST_BIN" "\$@"
EOF
	chmod +x "$TRASH_DIRECTORY/.bin/git"
	# Also write a 'grit' wrapper
	cat >"$TRASH_DIRECTORY/.bin/grit" <<EOF
#!/bin/sh
exec "$GUST_BIN" "\$@"
EOF
	chmod +x "$TRASH_DIRECTORY/.bin/grit"
	# Prepend .bin to PATH so every subshell sees 'git' -> grit
	export PATH="$TRASH_DIRECTORY/.bin:$PATH"
	# cd into trash so each test starts with a clean cwd
	cd "$TRASH_DIRECTORY" || exit 1
	# initialise a git repo like upstream test-lib does
	"$GUST_BIN" init -q || exit 1
}

# Allow tests to use $HOME (set before setup_trash so git config works)
HOME="$TRASH_DIRECTORY"
export HOME

# Default author/committer identity for tests
GIT_AUTHOR_NAME="Test Author"
GIT_AUTHOR_EMAIL="test@example.com"
GIT_COMMITTER_NAME="Test Committer"
GIT_COMMITTER_EMAIL="test@example.com"
export GIT_AUTHOR_NAME GIT_AUTHOR_EMAIL GIT_COMMITTER_NAME GIT_COMMITTER_EMAIL

# GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME support (before init)
if test -n "$GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME"
then
	mkdir -p "$TRASH_DIRECTORY"
	HOME="$TRASH_DIRECTORY" "$GUST_BIN" config --global init.defaultBranch "$GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME"
fi

setup_trash

# Quiet git/grit unless TEST_VERBOSE is set
if test -z "$TEST_VERBOSE"
then
	GIT_QUIET=-q
else
	GIT_QUIET=
fi

# Common constants
ZERO_OID=0000000000000000000000000000000000000000
SQ="'"
LF='
'
export ZERO_OID SQ LF

# ── helpers used by test bodies ──────────────────────────────────────────────

test_write_lines () {
	printf '%s\n' "$@"
}

test_grep () {
	local negate=""
	if test "$1" = "!"
	then
		negate="!"
		shift
	fi
	if test "$1" = "-e"
	then
		shift
	fi
	if test -n "$negate"
	then
		! grep "$@"
	else
		grep "$@"
	fi
}

test_config () {
	git config "$1" "$2" &&
	test_when_finished "git config --unset '$1'"
}

test_path_is_file () { test -f "$1"; }
test_path_is_dir  () { test -d "$1"; }
test_path_is_missing () { ! test -e "$1"; }

# test_line_count OP N FILE
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

# Evaluate $2 and check $1 == stdout.
test_cmp () {
	diff -u "$1" "$2"
}

# ── core test functions ───────────────────────────────────────────────────────

test_expect_success () {
	local description="$1"
	local commands="$2"
	test_count=$(($test_count + 1))

	# Run in a subshell so each test starts clean
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
