#!/bin/sh
# Ported from git/t/t3512-cherry-pick-submodule.sh
# Cherry-pick basic operations

test_description='cherry-pick basic operations'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	echo base >file &&
	git add file &&
	git commit -m "base" &&
	git tag base &&

	git checkout -b side &&
	echo side >file2 &&
	git add file2 &&
	git commit -m "add file2" &&
	git tag side-tag
'

test_expect_success 'cherry-pick from side branch to master' '
	git checkout master &&
	git cherry-pick side-tag &&
	test_path_is_file file2 &&
	test "$(cat file2)" = "side"
'

test_expect_success 'cherry-pick preserves file content' '
	test "$(cat file)" = "base" &&
	test "$(cat file2)" = "side"
'

test_expect_success 'cherry-pick creates proper commit' '
	git log --format=%s -n1 >actual &&
	echo "add file2" >expect &&
	test_cmp expect actual
'

test_done
