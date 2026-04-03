#!/bin/sh
# Ported from git/t/t5520-pull.sh
# Basic pull tests

test_description='git pull basic tests'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	echo file >file &&
	git add file &&
	git commit -m original
'

test_expect_success 'clone and pull setup' '
	git clone . cloned &&
	(
		cd cloned &&
		git config user.email "test@example.com" &&
		git config user.name "Test User"
	)
'

test_expect_success 'pull updates working tree' '
	echo "updated content" >file &&
	git commit -a -m "updated" &&
	(
		cd cloned &&
		git pull origin main &&
		echo "updated content" >expect &&
		cat file >actual &&
		test_cmp expect actual
	)
'

test_expect_success 'pull --ff-only succeeds on fast-forward' '
	echo "ff content" >file &&
	git commit -a -m "ff update" &&
	(
		cd cloned &&
		git pull --ff-only origin main &&
		echo "ff content" >expect &&
		cat file >actual &&
		test_cmp expect actual
	)
'

test_expect_success 'pull is fetch + merge' '
	echo "another update" >file &&
	git commit -a -m "another" &&
	(
		cd cloned &&
		old_head=$(git rev-parse HEAD) &&
		git pull origin main &&
		new_head=$(git rev-parse HEAD) &&
		test "$old_head" != "$new_head"
	)
'

test_expect_success 'pull -q is quiet' '
	echo "quiet update" >file &&
	git commit -a -m "quiet" &&
	(
		cd cloned &&
		git pull -q origin main 2>err &&
		test_must_be_empty err
	)
'

test_done
