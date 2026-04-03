#!/bin/sh

test_description='Test ref update operations'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup' '
	git commit --allow-empty -m initial &&
	COMMIT_OID=$(git rev-parse HEAD)
'

test_expect_success 'update-ref creates ref' '
	COMMIT_OID=$(git rev-parse HEAD) &&
	git update-ref refs/heads/something $COMMIT_OID &&
	git rev-parse refs/heads/something >actual &&
	echo $COMMIT_OID >expect &&
	test_cmp expect actual
'

test_expect_success 'update-ref can delete ref' '
	COMMIT_OID=$(git rev-parse HEAD) &&
	git update-ref refs/heads/to-delete $COMMIT_OID &&
	git update-ref -d refs/heads/to-delete &&
	test_must_fail git rev-parse --verify refs/heads/to-delete
'

test_done
