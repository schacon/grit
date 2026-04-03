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
		A_OID=$(git rev-parse HEAD) &&
		echo B >file && git add file && git commit -m B &&
		B_OID=$(git rev-parse HEAD) &&
		echo C >file && git add file && git commit -m C &&
		C_OID=$(git rev-parse HEAD)
	)
'

test_expect_success 'reflog delete --updateref HEAD@{0}' '
	cp -R repo copy &&
	(
		cd copy &&
		C_OID=$(git rev-parse HEAD) &&
		git reset --hard HEAD~ &&
		git reflog delete --updateref HEAD@{0} &&
		echo "$C_OID" >expect &&
		git rev-parse HEAD >actual &&
		test_cmp expect actual
	)
'

test_expect_success 'reflog delete --updateref HEAD@{1}' '
	cp -R repo copy2 &&
	(
		cd copy2 &&
		git reset --hard HEAD~ &&
		B_OID=$(git rev-parse HEAD) &&
		git reflog delete --updateref HEAD@{1} &&
		echo "$B_OID" >expect &&
		git rev-parse HEAD >actual &&
		test_cmp expect actual
	)
'

test_done
