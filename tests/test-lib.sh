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
# Use a per-test trash directory to avoid interference between tests.
# Derive from the test script name (e.g., t4050-diff.sh -> trash.t4050-diff)
_test_basename="$(basename "$0" .sh)"
TRASH_DIRECTORY="${TRASH_DIRECTORY:-$TEST_DIRECTORY/trash.$_test_basename}"
# BIN_DIRECTORY lives *outside* the working tree so that `git clean -x`
# (used in pristine_detach and similar helpers) cannot remove the wrapper
# scripts.  Using a sibling directory keeps things self-contained.
BIN_DIRECTORY="${TEST_DIRECTORY}/bin.${_test_basename}"
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
	if test -d "$TRASH_DIRECTORY"; then
		chmod -R u+rwx "$TRASH_DIRECTORY" 2>/dev/null
		rm -rf "$TRASH_DIRECTORY" 2>/dev/null
		# If rm -rf failed (e.g. locked files), try harder
		if test -d "$TRASH_DIRECTORY"; then
			find "$TRASH_DIRECTORY" -type f -exec chmod u+w {} + 2>/dev/null
			find "$TRASH_DIRECTORY" -type d -exec chmod u+rwx {} + 2>/dev/null
			rm -rf "$TRASH_DIRECTORY"
		fi
	fi
	mkdir -p "$TRASH_DIRECTORY"
	# BIN_DIRECTORY is outside the working tree so git clean -x cannot remove it
	mkdir -p "$BIN_DIRECTORY"
	# Write a 'git' wrapper script that calls grit
	cat >"$BIN_DIRECTORY/git" <<EOF
#!/bin/sh
exec "$GUST_BIN" "\$@"
EOF
	chmod +x "$BIN_DIRECTORY/git"
	# Also write a 'grit' wrapper
	cat >"$BIN_DIRECTORY/grit" <<EOF
#!/bin/sh
exec "$GUST_BIN" "\$@"
EOF
	chmod +x "$BIN_DIRECTORY/grit"
	# Write a 'scalar' wrapper
	cat >"$BIN_DIRECTORY/scalar" <<EOF
#!/bin/sh
exec "$GUST_BIN" scalar "\$@"
EOF
	chmod +x "$BIN_DIRECTORY/scalar"
	# Prepend BIN_DIRECTORY to PATH so every subshell sees 'git' → grit
PATH="$TEST_DIRECTORY:$PATH"
	export PATH="$BIN_DIRECTORY:$PATH"
	# cd into trash so each test starts with a clean cwd
	cd "$TRASH_DIRECTORY" || exit 1

	# Initialize a git repository in the trash directory (like upstream)
	if test -z "$TEST_NO_CREATE_REPO"
	then
		"$GUST_BIN" init >/dev/null 2>&1 ||
			echo "warning: could not git init trash directory" >&2
		"$GUST_BIN" config user.name "Test User" 2>/dev/null
		"$GUST_BIN" config user.email "test@example.com" 2>/dev/null

	fi
}

setup_trash

# Persist test_tick across subshell boundaries via a state file.
# Store inside .git/ so the file is never tracked by git.
_TICK_FILE="$TRASH_DIRECTORY/.git/.test_tick"

test_tick () {
	if test -z "${test_tick+set}"
	then
		# Try to load from file (survives subshell boundaries)
		if test -f "$_TICK_FILE"
		then
			test_tick=$(cat "$_TICK_FILE")
			test_tick=$(($test_tick + 60))
		else
			test_tick=1112911993
		fi
	else
		test_tick=$(($test_tick + 60))
	fi
	echo "$test_tick" >"$_TICK_FILE"
	GIT_COMMITTER_DATE="$test_tick -0700"
	GIT_AUTHOR_DATE="$test_tick -0700"
	export GIT_COMMITTER_DATE GIT_AUTHOR_DATE
}

# Allow tests to use $HOME — isolate from real user config
HOME="$TRASH_DIRECTORY"
XDG_CONFIG_HOME="$TRASH_DIRECTORY/.config"
export HOME XDG_CONFIG_HOME

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
	local negate=""
	local invert=""
	while test $# -gt 0; do
		case "$1" in
		-e) shift; break ;;
		!) negate=1; shift ;;
		-v) invert="-v"; shift ;;
		--) shift; break ;;
		-*) shift ;;
		*) break ;;
		esac
	done
	local pattern="$1"
	shift
	if test -n "$negate"
	then
		! grep "$pattern" "$@"
	else
		grep $invert "$pattern" "$@"
	fi
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

test_set_editor () {
	FAKE_EDITOR="$1"
	export FAKE_EDITOR
	EDITOR='"$FAKE_EDITOR"'
	export EDITOR
}

test_set_sequence_editor () {
	FAKE_SEQUENCE_EDITOR="$1"
	export FAKE_SEQUENCE_EDITOR
	GIT_SEQUENCE_EDITOR='"$FAKE_SEQUENCE_EDITOR"'
	export GIT_SEQUENCE_EDITOR
}

test_config () {
	local key="$1" val="$2"
	git config "$key" "$val" &&
	test_when_finished "git config --unset '$key'"
}

test_config_global () {
	local key="$1" val="$2"
	git config --global "$key" "$val" &&
	test_when_finished "git config --global --unset '$key'"
}

test_unconfig () {
	git config --unset-all "$@" 2>/dev/null
	return 0
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
		# If unsetting test_tick, also remove the persistence file
		if test "$1" = "test_tick" && test -n "${_TICK_FILE:-}"
		then
			rm -f "$_TICK_FILE"
		fi
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

# test_oid helpers — support SHA-1 only for now
test_oid () {
	case "$1" in
	numeric) echo "1234567890123456789012345678901234567890" ;;
	oid_version) echo "1" ;;
	rawsz) echo "20" ;;
	hexsz) echo "40" ;;
	algo) echo "sha1" ;;
	zero) echo "$ZERO_OID" ;;
	*) echo "unknown-oid" ;;
	esac
}

test_oid_cache () {
	# consume and ignore stdin
	cat >/dev/null
}

# CR/LF helpers
q_to_nul () {
	tr 'Q' '\000'
}

q_to_cr () {
	tr Q '\015'
}

q_to_tab () {
	tr Q '\011'
}

append_cr () {
	sed -e 's/$/Q/' | tr Q '\015'
}

remove_cr () {
	tr '\015' Q | sed -e 's/Q$//'
}

# test_dir_is_empty DIR
test_dir_is_empty () {
	test_path_is_dir "$1" &&
	if test -n "$(ls -a1 "$1" | grep -E -v '^\.\.$|^\.$')"
	then
		echo "Directory '$1' is not empty, it contains:"
		ls -la "$1"
		return 1
	fi
}

# test_bool_env VAR DEFAULT
test_bool_env () {
	local val="$(eval echo \$$1)"
	if test -z "$val"
	then
		val="$2"
	fi
	case "$val" in
	true|yes|1) return 0 ;;
	false|no|0) return 1 ;;
	*) return 1 ;;
	esac
}

# skip_all — set by tests that want to skip everything
skip_all=""

# test_ln_s_add TARGET LINK — create symlink and git add
test_ln_s_add () {
	ln -s "$1" "$2" &&
	git add "$2"
}

# test_cmp_rev [!] REV1 REV2
test_cmp_rev () {
	local _negate=""
	if test "$1" = "!"
	then
		_negate=1
		shift
	fi
	local r1 r2
	r1=$(git rev-parse --verify "$1") &&
	r2=$(git rev-parse --verify "$2") &&
	if test -n "$_negate"
	then
		if test "$r1" != "$r2"
		then
			return 0
		else
			echo >&2 "test_cmp_rev: $1 ($r1) == $2 ($r2) (expected different)"
			return 1
		fi
	else
		if test "$r1" = "$r2"
		then
			return 0
		else
			echo >&2 "test_cmp_rev: $1 ($r1) != $2 ($r2)"
			return 1
		fi
	fi
}

# test_unconfig KEY...
test_unconfig () {
	while test $# -gt 0; do
		git config --unset-all "$1" 2>/dev/null
		shift
	done
	return 0
}

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
	local _p="$1"
	# Handle negation: !PREREQ means "PREREQ is NOT set"
	if test "${_p#!}" != "$_p"; then
		local _neg="${_p#!}"
		! test_have_prereq "$_neg"
		return $?
	fi
	case "$_p" in
	POSIXPERM) return 0 ;;
	SYMLINKS)  return 0 ;;
	PIPE)      command -v mkfifo >/dev/null 2>&1 && return 0 ; return 1 ;;
	SANITY)    return 0 ;;
	FUNNYNAMES) return 0 ;;
	FILEMODE)  return 0 ;;
	COLON_DIR) return 0 ;;
	BSLASHPSPEC) return 0 ;;
	MINGW)     return 1 ;;  # Not on Windows
	CYGWIN)    return 1 ;;  # Not on Cygwin
	PERL)      command -v perl >/dev/null 2>&1 && return 0 ; return 1 ;;
	PERL_TEST_HELPERS) command -v perl >/dev/null 2>&1 && return 0 ; return 1 ;;
	GZIP)      command -v gzip >/dev/null 2>&1 && return 0 ; return 1 ;;
	FAKENC)    perl -MIO::Socket::INET -e 'exit 0' 2>/dev/null && return 0 ; return 1 ;;
	CGIPASSAUTH) return 1 ;; # Not supported by test-httpd
	*)
		# Check dynamic prereqs set by test_set_prereq
		local _var="_prereq_${_p}"
		eval "test \"\${${_var}:-}\" = set"
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

PERL_PATH=${PERL_PATH:-$(command -v perl 2>/dev/null || echo /usr/bin/perl)}
export PERL_PATH

# test_set_port VAR — assign a random port (or use existing value)
test_set_port () {
	local _varname="$1"
	eval "local _existing=\${${_varname}:-}"
	if test -z "$_existing"
	then
		# Pick a random port in the ephemeral range
		eval "${_varname}=$((10000 + (RANDOM % 50000)))"
	fi
}

# test_skip_or_die VAR MSG — skip test or die based on env var
test_skip_or_die () {
	if test_bool_env "$1" false
	then
		error "$2"
	fi
	skip_all="$2"
	test_done
}

# error MSG — print an error and exit
error () {
	echo "error: $*" >&2
	exit 1
}

# test_env — run command with additional env vars
# Usage: test_env VAR=VALUE ... command args
# Works with both binaries and shell functions
test_env () {
	local _te_vars=""
	while test $# -gt 0; do
		case "$1" in
		*=*)
			export "$1"
			_te_vars="$_te_vars $1"
			shift
			;;
		*)
			break
			;;
		esac
	done
	"$@"
	local _te_ret=$?
	# Unset the exported variables
	for _te_v in $_te_vars; do
		unset "${_te_v%%=*}"
	done
	return $_te_ret
}

# test_lazy_prereq NAME SCRIPT — define a prereq checked lazily
test_lazy_prereq () {
	if eval "$2" >/dev/null 2>&1
	then
		test_set_prereq "$1"
	fi
}

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
		if test -d "$indir/.git"
		then
			hook_dir="$indir/.git/hooks"
		else
			# bare repo
			hook_dir="$indir/hooks"
		fi
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

test_commit () {
	local notick= signoff= indir= tag=yes message= file= contents= author=
	while test $# != 0
	do
		case "$1" in
		--notick) notick=yes; shift ;;
		--signoff) signoff="$1"; shift ;;
		--no-tag) tag=; shift ;;
		--author) author="$2"; shift 2 ;;
		-C) indir="$2"; shift 2 ;;
		--append) shift ;; # accepted but ignored for compat
		--printf) shift ;; # accepted but ignored for compat
		*) break ;;
		esac
	done
	message="${1:?test_commit}" && shift
	file="${1:-$message.t}" && { test $# -gt 0 && shift || true; }
	contents="${1:-$message}" && { test $# -gt 0 && shift || true; }
	(
		test -n "$indir" && cd "$indir"
		printf '%s' "$contents" >"$file" &&
		git add "$file" &&
		if test -z "$notick"; then
			test_tick
		fi &&
		git commit -q ${signoff:+$signoff} ${author:+--author "$author"} -m "$message" &&
		if test -n "$tag"; then
			git tag "$message"
		fi
	)
}

test_merge () {
	local message="${1:?test_merge}" && shift
	test_tick &&
	git merge -m "$message" "$@" &&
	git tag "$message"
}

test_commit_bulk () {
	local indir= ref=HEAD n=
	while test $# != 0
	do
		case "$1" in
		-C) indir="$2"; shift 2 ;;
		--ref) ref="$2"; shift 2 ;;
		*) n="$1"; shift; break ;;
		esac
	done
	(
		test -n "$indir" && cd "$indir"
		local i=1
		while test "$i" -le "$n"
		do
			local message="commit $i"
			test_tick &&
			echo "$message" >"bulk-$i.t" &&
			git add "bulk-$i.t" &&
			git commit -m "$message" || return 1
			i=$((i + 1))
		done
	)
}

test_chmod () {
	local mode="$1" file="$2"
	if test "$mode" = "+x"
	then
		git update-index --chmod=+x "$file"
	else
		git update-index --chmod=-x "$file"
	fi
}

debug () {
	"$@"
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

	# Run in a subshell so each test starts clean.
	# Source any previously-exported variables so they persist across tests.
	# Run test body in the current shell (like upstream) so that
	# variables set in one test persist to subsequent tests.
	# We save and restore 'set -e' state since eval doesn't
	# propagate exit-on-error the way a subshell does.
	local _old_cwd="$PWD"
	cd "$TRASH_DIRECTORY" || return 1
	# Run with errexit but capture result.
	# Wrap in a function to localize set -e.
	_test_eval_result=0
	_test_eval_inner () {
		set -e
		eval "$1"
	}
	_twf_cmd=""
	_test_eval_inner "$commands" 2>&1 || _test_eval_result=$?
	local result=$_test_eval_result
	# Ensure errexit is off at top level
	set +e
	# Run test_when_finished cleanups (like upstream's per-test subshell EXIT trap).
	if test -n "${_twf_cmd+set}"
	then
		eval "$_twf_cmd" 2>/dev/null
		_twf_cmd=
		trap - EXIT
	fi
	cd "$_old_cwd" 2>/dev/null || true

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
		echo >&2 "BUG: test_expect_failure requires 2 or 3 arguments, got $#"
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

	local _exports_file="$TRASH_DIRECTORY/.test-exports"
	(
		set -e
		cd "$TRASH_DIRECTORY" || exit 1
		test -f "$_exports_file" && . "$_exports_file"
		eval "$commands"
	)
	local result=$?
	test -f "$_exports_file" && . "$_exports_file"

	# Sync test_tick state from file back to parent shell
	if test -f "$_TICK_FILE"
	then
		test_tick=$(cat "$_TICK_FILE")
		GIT_COMMITTER_DATE="$test_tick -0700"
		GIT_AUTHOR_DATE="$test_tick -0700"
		export GIT_COMMITTER_DATE GIT_AUTHOR_DATE
	elif test -n "${test_tick+set}"
	then
		unset test_tick GIT_COMMITTER_DATE GIT_AUTHOR_DATE 2>/dev/null
	fi

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

# Persist shell variables across test subshells.  Writes name=value pairs
# to a file that later subshells source on startup.  Usage:
#   test_export newf oldf f5id
test_export () {
	local _ef="$TRASH_DIRECTORY/.test-exports"
	for _var in "$@"; do
		local _val
		eval "_val=\"\$$_var\""
		# Remove any previous definition of this variable.
		if test -f "$_ef"; then
			sed -i "/^${_var}=/d" "$_ef"
		fi
		# Quote the value with single quotes (escape existing ones).
		local _escaped
		_escaped=$(printf '%s' "$_val" | sed "s/'/'\\\\''/g")
		printf "%s='%s'\n" "$_var" "$_escaped" >>"$_ef"
	done
}

test_when_finished () {
	# Register a command to run after the current test body completes.
	# Multiple calls accumulate in LIFO order (matching git's real test-lib),
	# so later registrations run first.
	_twf_cmd="$*${_twf_cmd:+; $_twf_cmd}"
}

test_must_be_empty () {
	if test -s "$1"
	then
		echo "file '$1' is not empty"
		cat "$1"
		return 1
	fi
}

test_ref_exists () {
	git rev-parse --verify -q "$1" >/dev/null 2>&1
}

test_ref_missing () {
	! test_ref_exists "$1"
}

test_path_is_file_not_symlink () {
	test -f "$1" && ! test -L "$1"
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

# Read up to "$1" bytes (or to EOF) from stdin and write them to stdout.
test_copy_bytes () {
	dd ibs=1 count="$1" 2>/dev/null
}

# Pkt-line helpers
packetize () {
	if test $# -gt 0
	then
		packet="$*"
		printf '%04x%s' "$((4 + ${#packet}))" "$packet"
	else
		test-tool pkt-line pack
	fi
}

packetize_raw () {
	test-tool pkt-line pack-raw-stdin
}

depacketize () {
	test-tool pkt-line unpack
}

# Build option stub — return reasonable defaults
build_option () {
	case "$1" in
	sizeof-size_t) echo 8 ;;
	*) echo "" ;;
	esac
}

# Extract remote HTTPS URLs from GIT_TRACE2_EVENT output
test_remote_https_urls() {
	grep -e '"event":"child_start".*"argv":\["git-remote-https",".*"\]' |
		sed -e 's/{"event":"child_start".*"argv":\["git-remote-https","//g' \
		    -e 's/"\]}//g'
}

# Convert Q to tab, Z to space.
qz_to_tab_space () {
	tr QZ '\011\040'
}

# Convert LF to NUL.
lf_to_nul () {
	tr '\012' '\000'
}

# Convert NUL to LF.
nul_to_q () {
	tr '\000' Q
}

# Append CR to each line.
append_cr () {
	sed -e 's/$/Q/' | tr Q '\015'
}

# Remove CR from each line.
remove_cr () {
	tr -d '\015'
}

test_done () {
	# Clean up the bin directory that lives outside the working tree
	rm -rf "$BIN_DIRECTORY" 2>/dev/null
	printf '\n'
	echo "# Tests: $test_count  Pass: $test_pass  Fail: $test_fail  Skip: $test_skip"
	if test $test_fail -gt 0
	then
		echo "${RED}FAILED:${RESET}$test_failures" >&2
		exit 1
	fi
	exit 0
}
