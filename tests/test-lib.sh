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
	# Prepend .bin to PATH so every subshell sees 'git' → grit
	export PATH="$TRASH_DIRECTORY/.bin:$PATH"
	# cd into trash so each test starts with a clean cwd
	cd "$TRASH_DIRECTORY" || exit 1
}

setup_trash

# Allow tests to use $HOME
HOME="$TRASH_DIRECTORY"
export HOME

# Quiet git/grit unless TEST_VERBOSE is set
if test -z "$TEST_VERBOSE"
then
	GIT_QUIET=-q
else
	GIT_QUIET=
fi

# ── helpers used by test bodies ──────────────────────────────────────────────

test_path_is_file () { test -f "$1"; }
test_path_is_dir  () { test -d "$1"; }
test_path_is_missing () { ! test -e "$1"; }

# test_line_count OP N FILE — assert wc -l $FILE $OP $N (e.g., = 1)
test_line_count () {
	local op="$1" count="$2" file="$3"
	local actual
	actual=$(wc -l <"$file")
	test "$actual" "$op" "$count"
}

# test_must_be_empty FILE — assert FILE has zero bytes
test_must_be_empty () { test ! -s "$1"; }

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

test_must_fail () {
	set +e
	if test "${TEST_HIDE_EXPECTED_FAIL_STDERR:-0}" = "1" && test -t 2
	then
		"$@" 2>/dev/null
	else
		"$@"
	fi
	status=$?
	set -e
	test $status -ne 0
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
