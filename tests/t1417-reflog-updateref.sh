#!/bin/sh

test_description='git reflog --updateref'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# grit does not yet support reflog delete --updateref or @{N} syntax.
# All tests are expected failures.

test_expect_success 'setup' '
	git init repo &&
	(
		cd repo &&
		git config user.name "Test" &&
		git config user.email "test@test.com" &&
		echo A >file && git add file && git commit -m A &&
		echo B >file && git add file && git commit -m B &&
		echo C >file && git add file && git commit -m C
	)
'

test_expect_failure 'reflog delete --updateref HEAD@{0}' '
	cp -R repo copy &&
	(
		cd copy &&
		git reset --hard HEAD~ &&
		git reflog delete --updateref HEAD@{0} &&
		git rev-parse B >expect &&
		git rev-parse HEAD >actual &&
		test_cmp expect actual
	)
'

test_expect_failure 'reflog delete --updateref HEAD@{1}' '
	cp -R repo copy2 &&
	(
		cd copy2 &&
		git reset --hard HEAD~ &&
		git reflog delete --updateref HEAD@{1} &&
		git rev-parse B >expect &&
		git rev-parse HEAD >actual &&
		test_cmp expect actual
	)
'

test_done
