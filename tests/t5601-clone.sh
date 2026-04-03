#!/bin/sh
# Ported from git/t/t5601-clone.sh
# Basic clone tests

test_description='clone'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	test_create_repo src &&
	(
		cd src &&
		>file &&
		git add file &&
		git commit -m initial &&
		echo 1 >file &&
		git add file &&
		git commit -m updated
	)
'

test_expect_success 'clone checks out files' '
	rm -fr dst &&
	git clone src dst &&
	test -f dst/file
'

test_expect_success 'clone creates intermediate directories' '
	git clone src long/path/to/dst &&
	test -f long/path/to/dst/file
'

test_expect_success 'clone creates intermediate directories for bare repo' '
	git clone --bare src long/path/to/bare/dst &&
	test -f long/path/to/bare/dst/config
'

test_expect_success 'clone -q is quiet' '
	rm -fr dst &&
	git clone -q src dst 2>err &&
	test_must_be_empty err
'

test_expect_success 'clone to destination with trailing /' '
	rm -fr dst &&
	git clone src dst/ &&
	test -f dst/file
'

test_expect_success 'clone --no-checkout' '
	rm -fr dst &&
	git clone -n src dst &&
	test_path_is_missing dst/file &&
	test -d dst/.git
'

test_expect_success 'clone sets up remote tracking' '
	rm -fr dst &&
	git clone src dst &&
	(cd dst && git remote -v) >output &&
	test_grep "origin" output
'

test_expect_success 'clone config has correct remote.origin.url' '
	rm -fr dst &&
	git clone src dst &&
	(cd dst && git config remote.origin.url) >actual &&
	# URL should reference src somehow
	test_grep "src" actual
'

test_expect_success 'clone config has correct remote.origin.fetch' '
	rm -fr dst &&
	git clone src dst &&
	echo "+refs/heads/*:refs/remotes/origin/*" >expect &&
	(cd dst && git config remote.origin.fetch) >actual &&
	test_cmp expect actual
'

test_expect_success 'clone sets HEAD to main' '
	rm -fr dst &&
	git clone src dst &&
	echo "ref: refs/heads/main" >expect &&
	cat dst/.git/HEAD >actual &&
	test_cmp expect actual
'

test_expect_success 'clone with multiple branches' '
	(cd src && git checkout -b other && echo other >other && git add other && git commit -m other && git checkout main) &&
	rm -fr dst &&
	git clone src dst &&
	(cd dst && git branch -r) >actual &&
	test_grep "origin/main" actual &&
	test_grep "origin/other" actual
'

test_expect_success 'clone preserves file content' '
	rm -fr dst &&
	git clone src dst &&
	echo 1 >expect &&
	cat dst/file >actual &&
	test_cmp expect actual
'

test_done
