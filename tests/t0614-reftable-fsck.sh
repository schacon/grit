#!/bin/sh

test_description='Test reftable backend consistency check'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# grit supports refs verify but not full reftable fsck.

test_expect_success 'setup reftable repo' '
	git init --ref-format=reftable repo &&
	(
		cd repo &&
		git config user.name "Test" &&
		git config user.email "test@test.com" &&
		echo content >file &&
		git add file &&
		git commit -m initial
	)
'

test_expect_success 'no errors on a well-formed reftable repository' '
	(
		cd repo &&
		git refs verify 2>err &&
		test_must_be_empty err
	)
'

test_expect_success 'invalid symref gets reported' '
	rm -rf repo2 &&
	git init repo2 &&
	(
		cd repo2 &&
		git config user.name "Test" &&
		git config user.email "test@test.com" &&
		echo content >file &&
		git add file &&
		git commit -m initial &&
		git symbolic-ref refs/heads/symref garbage &&
		test_must_fail git refs verify 2>err &&
		grep "invalid" err
	)
'

test_done
