# Upstream-compatible TAP (sourced from test-lib.sh after test_when_finished is defined).
# Requires: test-lib-harness.sh (match_*), TEST_NAME, TEST_NUMBER, this_test, GIT_SKIP_TESTS,
# run_list, verbose, verbose_only, immediate, invert_exit_code, trace, exec 3/4.

# Counters aligned with git/t/test-lib.sh
test_success=${test_success:-0}
test_failure=${test_failure:-0}
test_fixed=${test_fixed:-0}
test_broken=${test_broken:-0}

say() {
	printf '%s\n' "$*" >&3
}

last_verbose=t
maybe_setup_verbose() {
	test -z "$verbose_only" && return
	if match_pattern_list "$test_count" "$verbose_only"
	then
		exec 4>&2 3>&2
		test -z "$last_verbose" && echo >&3 ""
		verbose=t
	else
		exec 4>/dev/null 3>/dev/null
		verbose=
	fi
	last_verbose=$verbose
}

maybe_teardown_verbose() {
	test -z "$verbose_only" && return
	exec 4>/dev/null 3>/dev/null
	verbose=
}

test_skip() {
	description="$1"
	to_skip=
	skipped_reason=
	if match_pattern_list "$this_test.$test_count" "$GIT_SKIP_TESTS"
	then
		to_skip=t
		skipped_reason="GIT_SKIP_TESTS"
	fi
	if test -z "$to_skip" && test -n "$run_list" &&
		! match_test_selector_list '--run' "$description" "$test_count" "$run_list"
	then
		to_skip=t
		skipped_reason="--run"
	fi
	if test -z "$to_skip" && test -n "$test_prereq" &&
		! test_have_prereq "$test_prereq"
	then
		to_skip=t
		of_prereq=
		if test "$missing_prereq" != "$test_prereq"
		then
			of_prereq=" of $test_prereq"
		fi
		skipped_reason="missing $missing_prereq${of_prereq}"
	fi
	case "$to_skip" in
	t)
		printf '%sok %d # skip %s (%s)%s\n' "$YELLOW" "$test_count" "$description" "$skipped_reason" "$RESET"
		test_skip=$((test_skip + 1))
		test_pass=$((test_pass + 1))
		return 0
		;;
	*)
		return 1
		;;
	esac
}

test_expect_success() {
	prereq=""
	description=""
	commands=""
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
	if test "$commands" = "-"
	then
		commands="$(cat)"
	fi
	test_count=$((test_count + 1))
	maybe_setup_verbose
	test_prereq=$prereq
	export test_prereq
	missing_prereq=
	if test_skip "$description"
	then
		maybe_teardown_verbose
		return 0
	fi
	_test_eval_result=0
	_test_eval_inner() {
		set -e
		# Match upstream git test-lib: each test starts from the trash directory.
		# Otherwise a prior test's `cd repo` leaves the shell nested and breaks
		# later blocks that run `cd repo && ...`.
		cd "$TRASH_DIRECTORY" || exit 1
		eval "$1"
	}
	_twf_cmd=""
	test -z "$verbose" || say "expecting success of $TEST_NUMBER.$test_count '$description': $commands"
	# Each block assumes cwd is the trash root (like upstream git tests). A prior
	# block may have left the shell in a subdirectory (e.g. setup ends in `repo/`).
	if test -n "$TRASH_DIRECTORY"
	then
		cd "$TRASH_DIRECTORY" || exit 1
	fi
	_test_eval_inner "$commands" </dev/null 2>&1 || _test_eval_result=$?
	result=$_test_eval_result
	set +e
	if test -n "${_twf_cmd+set}"
	then
		eval "$_twf_cmd" 2>/dev/null
		_twf_cmd=
		trap - EXIT
	fi
	maybe_teardown_verbose
	if test "$result" -eq 0
	then
		test_success=$((test_success + 1))
		test_pass=$((test_pass + 1))
		printf '%sok %d - %s%s\n' "$GREEN" "$test_count" "$description" "$RESET"
	else
		test_failure=$((test_failure + 1))
		test_fail=$((test_fail + 1))
		test_failures="$test_failures
  FAIL $test_count: $description"
		printf '%snot ok %d - %s%s\n' "$RED" "$test_count" "$description" "$RESET"
		printf '%s\n' "$commands" | sed -e 's/^/#	/'
		if test -n "$immediate"
		then
			echo "1..$test_count"
			exit 1
		fi
	fi
}

test_expect_failure() {
	prereq=""
	description=""
	commands=""
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
	if test "$commands" = "-"
	then
		commands="$(cat)"
	fi
	test_count=$((test_count + 1))
	maybe_setup_verbose
	test_prereq=$prereq
	export test_prereq
	missing_prereq=
	if test_skip "$description"
	then
		maybe_teardown_verbose
		return 0
	fi
	test -z "$verbose" || say "checking known breakage of $TEST_NUMBER.$test_count '$description': $commands"
	_exports_file="$TRASH_DIRECTORY/.test-exports"
	(
		set -e
		cd "$TRASH_DIRECTORY" || exit 1
		test -f "$_exports_file" && . "$_exports_file"
		eval "$commands" </dev/null
	)
	result=$?
	test -f "$_exports_file" && . "$_exports_file"
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
	maybe_teardown_verbose
	if test "$result" -ne 0
	then
		test_broken=$((test_broken + 1))
		printf '%snot ok %d - %s # TODO known breakage%s\n' "$YELLOW" "$test_count" "$description" "$RESET"
	else
		test_fixed=$((test_fixed + 1))
		printf '%sok %d - %s # TODO known breakage vanished%s\n' "$RED" "$test_count" "$description" "$RESET"
	fi
}

test_must_fail_acceptable() {
	if test "$1" = "env"
	then
		shift
		while test $# -gt 0
		do
			case "$1" in
			*?=*)
				shift
				;;
			*)
				break
				;;
			esac
		done
	fi
	if test "$1" = "nongit"
	then
		shift
	fi
	case "$1" in
	git|__git*|scalar|test-tool|test_terminal)
		return 0
		;;
	*)
		return 1
		;;
	esac
}

test_must_fail() {
	_test_ok=
	case "$1" in
	ok=*)
		_test_ok=${1#ok=}
		shift
		;;
	esac
	if ! test_must_fail_acceptable "$@"
	then
		echo "test_must_fail: only 'git' is allowed: $*" >&2
		return 1
	fi
	set +e
	"$@"
	exit_code=$?
	set -e
	if test "$exit_code" -eq 0 && ! echo "$_test_ok" | grep -q success
	then
		echo "test_must_fail: command succeeded: $*" >&2
		return 1
	elif test "$exit_code" -gt 129 && test "$exit_code" -le 192
	then
		echo "test_must_fail: died by signal $(($exit_code - 128)): $*" >&2
		return 1
	elif test "$exit_code" -eq 127
	then
		echo "test_must_fail: command not found: $*" >&2
		return 1
	elif test "$exit_code" -eq 126
	then
		echo "test_must_fail: valgrind error: $*" >&2
		return 1
	fi
	return 0
}

test_done() {
	rm -rf "$BIN_DIRECTORY" 2>/dev/null
	if test -n "$skip_all"
	then
		echo "1..0 # SKIP $skip_all"
		if test -n "${GRIT_TEST_LIB_SUMMARY:-}"
		then
			echo "# Tests: 0  Pass: 0  Fail: 0  Skip: 0"
		fi
		exit 0
	fi
	if test "$test_fixed" != 0
	then
		echo "# $test_fixed known breakage(s) vanished; please update test(s)"
	fi
	if test "$test_broken" != 0
	then
		echo "# still have $test_broken known breakage(s)"
	fi
	if test "$test_broken" != 0 || test "$test_fixed" != 0
	then
		test_remaining=$((test_count - test_broken - test_fixed))
		msg="remaining $test_remaining test(s)"
	else
		test_remaining=$test_count
		msg="$test_count test(s)"
	fi
	case "$test_failure" in
	0)
		if test "$test_remaining" -gt 0
		then
			echo "# passed all $msg"
		fi
		echo "1..$test_count"
		;;
	*)
		echo "# failed $test_failure among $msg"
		echo "1..$test_count"
		;;
	esac
	if test -n "${GRIT_TEST_LIB_SUMMARY:-}"
	then
		echo "# Tests: $test_count  Pass: $test_pass  Fail: $test_fail  Skip: $test_skip"
	fi
	if test "$test_failure" -gt 0
	then
		exit 1
	fi
	if test "$test_fixed" != 0 && test -z "$invert_exit_code"
	then
		exit 1
	fi
	exit 0
}
