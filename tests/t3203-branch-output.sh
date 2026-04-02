#!/bin/sh
# Tests for 'grit branch' listing output format.
# Ported from git/t/t3203-branch-output.sh

test_description='grit branch listing output format'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repo with multiple branches' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&
	echo base >file &&
	git add file &&
	git commit -m "initial" &&
	git branch alpha &&
	git branch beta &&
	git branch gamma
'

test_expect_success 'branch lists all local branches' '
	cd repo &&
	git branch >actual &&
	grep "alpha" actual &&
	grep "beta" actual &&
	grep "gamma" actual &&
	grep "master" actual
'

test_expect_success 'current branch is marked with asterisk' '
	cd repo &&
	git branch >actual &&
	grep "^\* master" actual
'

test_expect_success 'non-current branches have no asterisk' '
	cd repo &&
	git branch >actual &&
	grep "^  alpha" actual &&
	grep "^  beta" actual
'

test_expect_success 'branch output is sorted alphabetically' '
	cd repo &&
	git branch >actual &&
	# Extract branch names, strip leading spaces and asterisk
	sed "s/^[* ] *//" actual >names &&
	sort names >sorted &&
	test_cmp sorted names
'

test_expect_success 'branch -v shows commit hash and subject' '
	cd repo &&
	git branch -v >actual &&
	# Each line should have: branch-name hash subject
	grep "alpha" actual | grep "initial" &&
	grep "master" actual | grep "initial"
'

test_expect_success 'branch -v shows short hash (7+ chars)' '
	cd repo &&
	git branch -v >actual &&
	# master line should contain a hex hash
	grep "master" actual | grep -E "[0-9a-f]{7}"
'

test_expect_success 'branch -vv shows verbose info' '
	cd repo &&
	git branch -vv >actual &&
	grep "master" actual &&
	grep "alpha" actual
'

test_expect_success 'branch --list works same as default listing' '
	cd repo &&
	git branch >expected &&
	git branch --list >actual &&
	test_cmp expected actual
'

test_expect_success 'branch after switching shows new current' '
	cd repo &&
	git checkout alpha &&
	git branch >actual &&
	grep "^\* alpha" actual &&
	! grep "^\* master" actual &&
	git checkout master
'

test_expect_success 'branch -r shows nothing with no remotes' '
	cd repo &&
	git branch -r >actual &&
	test_must_be_empty actual
'

test_expect_success 'branch -a lists same as branch when no remotes' '
	cd repo &&
	git branch >expected &&
	git branch -a >actual &&
	test_cmp expected actual
'

test_expect_success 'branch --show-current shows current branch name' '
	cd repo &&
	git branch --show-current >actual &&
	echo "master" >expected &&
	test_cmp expected actual
'

test_expect_success 'branch --show-current after checkout' '
	cd repo &&
	git checkout beta &&
	git branch --show-current >actual &&
	echo "beta" >expected &&
	test_cmp expected actual &&
	git checkout master
'

test_expect_success 'branch -v alignment: all lines have hash' '
	cd repo &&
	git branch -v >actual &&
	# Every non-empty line should contain a hex hash
	while IFS= read -r line; do
		test -z "$line" && continue
		echo "$line" | grep -qE "[0-9a-f]{7}" || {
			echo "Line without hash: $line"
			return 1
		}
	done <actual
'

test_expect_success 'setup: create branch with long name' '
	cd repo &&
	git branch a-very-long-branch-name-for-alignment-testing
'

test_expect_success 'branch -v with long name still shows hash' '
	cd repo &&
	git branch -v >actual &&
	grep "a-very-long-branch-name" actual | grep -E "[0-9a-f]{7}"
'

test_expect_success 'branch with no args is same as --list' '
	cd repo &&
	git branch >no_args &&
	git branch --list >with_list &&
	test_cmp no_args with_list
'

test_expect_success 'branch -q creates branch quietly' '
	cd repo &&
	git branch -q quiet-branch >actual 2>&1 &&
	test_must_be_empty actual
'

test_expect_success 'branch -d removes branch from listing' '
	cd repo &&
	git branch to-delete &&
	git branch >before &&
	grep "to-delete" before &&
	git branch -d to-delete &&
	git branch >after &&
	! grep "to-delete" after
'

test_expect_success 'branch -D force-deletes branch' '
	cd repo &&
	git branch force-delete &&
	git branch -D force-delete &&
	git branch >actual &&
	! grep "force-delete" actual
'

test_done
