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
	# Prepend .bin to PATH so every subshell sees 'git' → grit
	export PATH="$TRASH_DIRECTORY/.bin:$PATH"
	# cd into trash so each test starts with a clean cwd
	cd "$TRASH_DIRECTORY" || exit 1
}

setup_trash

# Allow tests to use $HOME
HOME="$TRASH_DIRECTORY"
export HOME

# Prevent tests from discovering enclosing repositories
GIT_CEILING_DIRECTORIES="$(dirname "$TRASH_DIRECTORY")"
export GIT_CEILING_DIRECTORIES

# Set default author/committer identity for all tests
GIT_AUTHOR_NAME="A U Thor"
GIT_AUTHOR_EMAIL="author@example.com"
GIT_COMMITTER_NAME="C O Mitter"
GIT_COMMITTER_EMAIL="committer@example.com"
export GIT_AUTHOR_NAME GIT_AUTHOR_EMAIL GIT_COMMITTER_NAME GIT_COMMITTER_EMAIL

# Quiet git/grit unless TEST_VERBOSE is set
if test -z "$TEST_VERBOSE"
then
	GIT_QUIET=-q
else
	GIT_QUIET=
fi

# ── constants ────────────────────────────────────────────────────────────────

ZERO_OID=0000000000000000000000000000000000000000
SQ="'"
LF='
'
export ZERO_OID SQ LF

if test -n "$GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME"
then
	git config --global init.defaultBranch "$GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME"
fi

# ── helpers used by test bodies ──────────────────────────────────────────────

test_path_is_file () { test -f "$1"; }
test_path_is_dir  () { test -d "$1"; }
test_path_is_missing () { ! test -e "$1"; }

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

sane_unset () {
	while test $# -gt 0; do
		unset "$1" 2>/dev/null
		shift
	done
}

test_seq () {
	local i="$1" end="${2:-}"
	if test -z "$end"; then
		end=$i
		i=1
	fi
	while test "$i" -le "$end"; do
		echo "$i"
		i=$(($i + 1))
	done
}

test_cmp_bin () {
	cmp "$@"
}

test_decode_color () {
	sed \
		-e 's/\x1b\[1m/<BOLD>/g' \
		-e 's/\x1b\[7m/<REVERSE>/g' \
		-e 's/\x1b\[30m/<BLACK>/g' \
		-e 's/\x1b\[31m/<RED>/g' \
		-e 's/\x1b\[32m/<GREEN>/g' \
		-e 's/\x1b\[33m/<YELLOW>/g' \
		-e 's/\x1b\[34m/<BLUE>/g' \
		-e 's/\x1b\[35m/<MAGENTA>/g' \
		-e 's/\x1b\[36m/<CYAN>/g' \
		-e 's/\x1b\[m/<RESET>/g' \
		-e 's/\x1b\[0m/<RESET>/g' \
		-e 's/\x1b\[[0-9;]*m//g'
}

OID_REGEX='[0-9a-f]{40,}'
export OID_REGEX

nongit () {
	local tmpdir
	tmpdir=$(mktemp -d) &&
	(
		cd "$tmpdir" &&
		GIT_CEILING_DIRECTORIES="$tmpdir" &&
		export GIT_CEILING_DIRECTORIES &&
		"$@"
	)
	local rc=$?
	rm -rf "$tmpdir"
	return $rc
}

test_i18ngrep () {
	test_grep "$@"
}

# test_line_count OP N FILE — assert wc -l $FILE $OP $N (e.g., = 1)
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

# test_must_be_empty FILE — assert FILE has zero bytes
test_must_be_empty () { test ! -s "$1"; }

test_have_prereq () {
	case "$1" in
	POSIXPERM) return 0 ;;
	SYMLINKS)  return 0 ;;
	PIPE)      command -v mkfifo >/dev/null 2>&1 && return 0 ; return 1 ;;
	SANITY)    return 0 ;;
	*)
		# Check dynamic prereqs set by test_set_prereq
		eval "test \"\${_prereq_$1:-}\" = set"
		return $?
		;;
	esac
}

test_set_prereq () {
	eval "_prereq_$1=set"
}

# TAR for tests that need it
TAR=${TAR:-tar}
export TAR

# write_script FILE [INTERPRETER] — write a script from stdin
write_script () {
	{
		echo "#!${2-/bin/sh}" &&
		cat
	} >"$1" &&
	chmod +x "$1"
}

# test_hook [--setup] HOOKNAME — write a hook script from stdin
test_hook () {
	local setup= indir=
	while test $# != 0
	do
		case "$1" in
		--setup)
			setup=t
			shift
			;;
		-C)
			indir="$2"
			shift 2
			;;
		*)
			break
			;;
		esac
	done
	local hook_dir
	if test -n "$indir"
	then
		hook_dir="$indir/.git/hooks"
	else
		hook_dir=".git/hooks"
	fi
	mkdir -p "$hook_dir" &&
	write_script "$hook_dir/$1"
}

# test_cmp_config [--default DEFAULT] EXPECTED [KEY...]
test_cmp_config () {
	local default=""
	if test "$1" = "--default"
	then
		default="$2"
		shift 2
	fi
	local expect="$1"
	shift
	local actual
	actual=$(git config "$@" 2>/dev/null) || actual="$default"
	if test "$expect" = "$actual"
	then
		return 0
	else
		echo >&2 "test_cmp_config: expected '$expect', got '$actual'"
		return 1
	fi
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
	local prereq=""
	local description
	local commands
	if test $# -eq 3
	then
		prereq="$1"
		description="$2"
		commands="$3"
	elif test $# -eq 2
	then
		description="$1"
		commands="$2"
	else
		echo >&2 "BUG: test_expect_success requires 2 or 3 arguments, got $#"
		return 1
	fi
	test_count=$(($test_count + 1))

	# Check prerequisites (comma-separated)
	if test -n "$prereq"
	then
		local _all_met=1
		local _save_IFS="$IFS"
		IFS=','
		for _p in $prereq
		do
			if ! test_have_prereq "$_p"
			then
				_all_met=0
				break
			fi
		done
		IFS="$_save_IFS"
		if test "$_all_met" = 0
		then
			test_pass=$(($test_pass + 1))
			test_skip=$(($test_skip + 1))
			if test -n "$TEST_VERBOSE"
			then
				printf '%sok %d - %s # SKIP (missing prereq %s)%s\n' "$YELLOW" "$test_count" "$description" "$prereq" "$RESET"
			else
				printf 's'
			fi
			return 0
		fi
	fi

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
	# Register a command to run when the current test's subshell exits.
	# Since each test_expect_success body runs in its own subshell, an EXIT
	# trap is the right hook.  Multiple calls accumulate.
	_twf_cmd="${_twf_cmd:+${_twf_cmd}; }$*"
	trap 'eval "$_twf_cmd"' EXIT
}

test_must_be_empty () {
	if test -s "$1"
	then
		echo "file '$1' is not empty"
		cat "$1"
		return 1
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

test_must_be_empty () {
	if test -s "$1"
	then
		echo >&2 "test_must_be_empty: file '$1' is not empty"
		return 1
	fi
	return 0
}

test_line_count () {
	local op="$1"
	local count="$2"
	local file="$3"
	local actual
	actual=$(wc -l <"$file")
	# trim whitespace
	actual=$(echo "$actual" | tr -d ' ')
	if test "$actual" "$op" "$count"
	then
		return 0
	else
		echo >&2 "test_line_count: expected $count lines ($op), got $actual in '$file'"
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
