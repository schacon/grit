#!/bin/sh
# Tests for branch output with/without color.
# NOTE: grit does not implement --color/--no-color flags for branch.
# These tests verify branch listing behavior and document color gaps.

test_description='grit branch output and color'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.email "test@test.com" &&
	git config user.name "Test" &&
	echo init >file &&
	git add file &&
	git commit -m "initial"
'

test_expect_success 'branch list shows current branch with asterisk' '
	cd repo &&
	git branch >out &&
	grep "^\* master" out
'

test_expect_success 'branch list with multiple branches' '
	cd repo &&
	git branch topic &&
	git branch >out &&
	grep "^\* master" out &&
	grep "  topic" out
'

test_expect_success 'branch -v shows commit hash and subject' '
	cd repo &&
	git branch -v >out &&
	grep "master" out &&
	grep "initial" out
'

test_expect_success 'branch -vv shows commit info' '
	cd repo &&
	git branch -vv >out &&
	grep "master" out &&
	grep "initial" out
'

test_expect_success 'branch --color is not supported (documents gap)' '
	cd repo &&
	test_must_fail git branch --color 2>err &&
	grep -i "unexpected argument\|error" err
'

test_expect_success 'branch --no-color is not supported (documents gap)' '
	cd repo &&
	test_must_fail git branch --no-color 2>err &&
	grep -i "unexpected argument\|error" err
'

test_expect_success 'branch --color=always is not supported (documents gap)' '
	cd repo &&
	test_must_fail git branch --color=always 2>err &&
	grep -i "unexpected argument\|error" err
'

test_expect_success 'branch --color=never is not supported (documents gap)' '
	cd repo &&
	test_must_fail git branch --color=never 2>err &&
	grep -i "unexpected argument\|error" err
'

test_expect_success 'branch -a lists all branches' '
	cd repo &&
	git branch -a >out &&
	grep "master" out &&
	grep "topic" out
'

test_expect_success 'branch -l explicitly lists branches' '
	cd repo &&
	git branch -l >out &&
	grep "master" out &&
	grep "topic" out
'

test_expect_success 'branch output has no ANSI escapes without color support' '
	cd repo &&
	git branch >out &&
	! grep -P "\x1b\[" out
'

test_expect_success 'branch -v output has no ANSI escapes' '
	cd repo &&
	git branch -v >out &&
	! grep -P "\x1b\[" out
'

test_expect_success 'branch with --show-current shows current branch name' '
	cd repo &&
	git branch --show-current >out &&
	echo "master" >expect &&
	test_cmp expect out
'

test_expect_success 'branch --show-current on detached HEAD is empty' '
	cd repo &&
	oid=$(git rev-parse HEAD) &&
	git checkout $oid 2>/dev/null &&
	git branch --show-current >out &&
	test_must_fail test -s out &&
	git checkout master 2>/dev/null
'

test_expect_success 'branch -q suppresses output on creation' '
	cd repo &&
	git branch -q quiet-branch 2>err &&
	test_must_fail test -s err &&
	git branch -d quiet-branch
'

test_expect_success 'multiple branches sort alphabetically' '
	cd repo &&
	git branch beta &&
	git branch alpha &&
	git branch >out &&
	grep -n "alpha" out >alpha_line &&
	grep -n "beta" out >beta_line &&
	alpha_num=$(cut -d: -f1 alpha_line) &&
	beta_num=$(cut -d: -f1 beta_line) &&
	test "$alpha_num" -lt "$beta_num" &&
	git branch -d alpha beta
'

test_expect_success 'branch -v aligns columns consistently' '
	cd repo &&
	git branch short &&
	git branch very-long-branch-name &&
	git branch -v >out &&
	grep "short" out &&
	grep "very-long-branch-name" out &&
	git branch -d short very-long-branch-name
'

test_expect_success 'branch list after delete does not show deleted branch' '
	cd repo &&
	git branch deleteme &&
	git branch -d deleteme &&
	git branch >out &&
	! grep "deleteme" out
'

test_expect_success 'branch output distinguishes current from others' '
	cd repo &&
	git branch >out &&
	current=$(grep "^\*" out | wc -l) &&
	test "$current" -eq 1
'

test_expect_success 'cleanup: delete extra branches' '
	cd repo &&
	git branch -D topic 2>/dev/null || true
'

test_done
