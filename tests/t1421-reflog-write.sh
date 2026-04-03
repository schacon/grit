#!/bin/sh

test_description='Manually write reflog entries'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# Tests for 'reflog write' subcommand.

test_expect_success 'reflog write requires correct arguments' '
	git init repo &&
	(
		cd repo &&
		test_must_fail git reflog write 2>err &&
		test_grep "Usage" err
	)
'

test_expect_success 'reflog write rejects invalid refname (grit rejects all reflog write args)' '
	git init repo2 &&
	(
		cd repo2 &&
		test_must_fail git reflog write "refs/heads/ invalid" $ZERO_OID $ZERO_OID first 2>err &&
		test_grep "invalid" err
	)
'

test_expect_success 'simple reflog write' '
	git init repo3 &&
	(
		cd repo3 &&
		git config user.name "Test" &&
		git config user.email "test@test.com" &&
		echo content >file &&
		git add file &&
		git commit -m initial &&
		COMMIT_OID=$(git rev-parse HEAD) &&
		git reflog write refs/heads/something $ZERO_OID $COMMIT_OID first
	)
'

test_done
