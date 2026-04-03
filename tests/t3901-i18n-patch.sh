#!/bin/sh
# Ported from git/t/t3901-i18n-patch.sh
# Format-patch and am basic tests

test_description='format-patch and am basic tests'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	echo initial >file &&
	git add file &&
	test_tick &&
	git commit -m "Initial commit" &&
	git tag initial &&

	echo modified >file &&
	git add file &&
	test_tick &&
	git commit -m "Modified file" &&
	git tag modified
'

test_expect_success 'format-patch creates patch file' '
	git format-patch HEAD~1 >patches &&
	test -s patches &&
	patch_file=$(cat patches) &&
	test -f "$patch_file"
'

test_expect_success 'format-patch --stdout outputs to stdout' '
	git format-patch --stdout HEAD~1 >patch &&
	grep "Subject:" patch &&
	grep "Modified file" patch
'

test_expect_success 'am applies patch' '
	git checkout modified &&
	git format-patch --stdout HEAD~1 >patch &&
	git checkout -b am-test initial &&
	git am <patch &&
	git diff --quiet modified -- &&
	git log --format=%s -n1 >actual &&
	echo "Modified file" >expect &&
	test_cmp expect actual
'

test_expect_success 'format-patch multiple commits' '
	git checkout main &&
	echo another >file2 &&
	git add file2 &&
	test_tick &&
	git commit -m "Add file2" &&
	git format-patch -2 >patches &&
	test_line_count = 2 patches
'

test_done
