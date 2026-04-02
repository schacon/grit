#!/bin/sh
#
# Tests for 'grit check-ref-format' — invalid ref names and edge cases.
# Ported subset from git/t/t1430-bad-ref-name.sh (upstream ~42 tests).

test_description='grit check-ref-format — ref name validation'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ---------------------------------------------------------------------------
# Valid ref names
# ---------------------------------------------------------------------------
test_expect_success 'valid: refs/heads/master' '
	git check-ref-format refs/heads/master
'

test_expect_success 'valid: refs/heads/feature/branch' '
	git check-ref-format refs/heads/feature/branch
'

test_expect_success 'valid: refs/tags/v1.0' '
	git check-ref-format refs/tags/v1.0
'

test_expect_success 'valid: refs/heads/a-b-c' '
	git check-ref-format refs/heads/a-b-c
'

test_expect_success 'valid: refs/heads/a.b' '
	git check-ref-format refs/heads/a.b
'

# ---------------------------------------------------------------------------
# Invalid: double dot (..)
# ---------------------------------------------------------------------------
test_expect_success 'invalid: double dot in component' '
	test_must_fail git check-ref-format refs/heads/mas..ter
'

test_expect_success 'invalid: double dot at start' '
	test_must_fail git check-ref-format refs/heads/..master
'

# ---------------------------------------------------------------------------
# Invalid: ends with .lock
# ---------------------------------------------------------------------------
test_expect_success 'invalid: component ending with .lock' '
	test_must_fail git check-ref-format refs/heads/master.lock
'

test_expect_success 'invalid: intermediate component ending with .lock' '
	test_must_fail git check-ref-format refs/heads.lock/master
'

# ---------------------------------------------------------------------------
# Invalid: trailing slash / empty component
# ---------------------------------------------------------------------------
test_expect_success 'invalid: trailing slash' '
	test_must_fail git check-ref-format refs/heads/
'

test_expect_success 'invalid: empty component (double slash)' '
	test_must_fail git check-ref-format refs/heads//master
'

# ---------------------------------------------------------------------------
# Invalid: special characters
# ---------------------------------------------------------------------------
test_expect_success 'invalid: space in ref name' '
	test_must_fail git check-ref-format "refs/heads/mas ter"
'

test_expect_success 'invalid: tilde in ref name' '
	test_must_fail git check-ref-format "refs/heads/mas~ter"
'

test_expect_success 'invalid: caret in ref name' '
	test_must_fail git check-ref-format "refs/heads/mas^ter"
'

test_expect_success 'invalid: colon in ref name' '
	test_must_fail git check-ref-format "refs/heads/mas:ter"
'

test_expect_success 'invalid: open bracket in ref name' '
	test_must_fail git check-ref-format "refs/heads/mas[ter"
'

test_expect_success 'invalid: backslash in ref name' '
	test_must_fail git check-ref-format "refs/heads/mas\\ter"
'

test_expect_success 'invalid: control char in ref name' '
	test_must_fail git check-ref-format "refs/heads/mas$(printf "\\007")ter"
'

# ---------------------------------------------------------------------------
# Invalid: dot at start or end of component
# ---------------------------------------------------------------------------
test_expect_success 'invalid: component starting with dot' '
	test_must_fail git check-ref-format refs/heads/.master
'

test_expect_success 'invalid: component ending with dot' '
	test_must_fail git check-ref-format refs/heads/master.
'

# ---------------------------------------------------------------------------
# Invalid: @{ sequence
# ---------------------------------------------------------------------------
test_expect_success 'invalid: @{ in ref name' '
	test_must_fail git check-ref-format "refs/heads/@{master}"
'

test_expect_success 'invalid: contains @{' '
	test_must_fail git check-ref-format "refs/heads/mas@{ter"
'

# ---------------------------------------------------------------------------
# Single-level refs (require --allow-onelevel)
# ---------------------------------------------------------------------------
test_expect_success 'single-level ref rejected without flag' '
	test_must_fail git check-ref-format master
'

test_expect_success 'single-level ref accepted with --allow-onelevel' '
	git check-ref-format --allow-onelevel master
'

test_expect_success 'single-level ref HEAD accepted with --allow-onelevel' '
	git check-ref-format --allow-onelevel HEAD
'

# ---------------------------------------------------------------------------
# --refspec-pattern
# ---------------------------------------------------------------------------
test_expect_success 'refspec pattern with single wildcard accepted' '
	git check-ref-format --refspec-pattern "refs/heads/*"
'

test_expect_success 'refspec pattern with double wildcard rejected' '
	test_must_fail git check-ref-format --refspec-pattern "refs/heads/*/*"
'

test_expect_success 'refspec pattern wildcard in middle accepted' '
	git check-ref-format --refspec-pattern "refs/*/master"
'

# ---------------------------------------------------------------------------
# --normalize
# ---------------------------------------------------------------------------
test_expect_success 'normalize collapses consecutive slashes' '
	result=$(git check-ref-format --normalize "refs///heads///master") &&
	test "$result" = "refs/heads/master"
'

test_expect_success 'normalize strips leading slash' '
	result=$(git check-ref-format --normalize "/refs/heads/master") &&
	test "$result" = "refs/heads/master"
'

test_expect_success 'normalize rejects invalid ref after normalizing' '
	test_must_fail git check-ref-format --normalize "refs/heads/mas..ter"
'

test_expect_success 'normalize rejects empty string' '
	test_must_fail git check-ref-format --normalize ""
'

# ---------------------------------------------------------------------------
# --branch
# ---------------------------------------------------------------------------
test_expect_success 'branch mode validates branch name' '
	result=$(git check-ref-format --branch "master") &&
	test "$result" = "master"
'

test_expect_success 'branch mode rejects invalid branch name' '
	test_must_fail git check-ref-format --branch "mas..ter"
'

test_expect_success 'branch mode rejects ref with space' '
	test_must_fail git check-ref-format --branch "my branch"
'

# ---------------------------------------------------------------------------
# Edge cases
# ---------------------------------------------------------------------------
test_expect_success 'invalid: asterisk without --refspec-pattern' '
	test_must_fail git check-ref-format "refs/heads/*"
'

test_expect_success 'valid: underscore in ref name' '
	git check-ref-format refs/heads/my_branch
'

test_expect_success 'valid: at sign (bare) in ref name' '
	git check-ref-format refs/heads/user@host
'

test_expect_success 'invalid: bare @ is rejected for multi-level' '
	test_must_fail git check-ref-format @
'

test_done
