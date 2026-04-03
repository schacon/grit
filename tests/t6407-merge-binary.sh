#!/bin/sh

test_description='ask merge to handle binary file conflicts'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test" &&
	printf "\\0initial" >m &&
	git add m &&
	test_tick &&
	git commit -m "initial" &&

	git branch side &&
	echo frotz >a &&
	git add a &&
	printf "\\0main-change" >m &&
	git add a m &&
	test_tick &&
	git commit -m "main adds some" &&

	git checkout side &&
	printf "\\0side-change" >m &&
	git add m &&
	test_tick &&
	git commit -m "side modifies" &&

	git tag anchor
'

test_expect_success 'merge with binary conflict fails' '
	cd repo &&
	rm -f a* m* &&
	git reset --hard anchor &&
	test_must_fail git merge main
'

test_expect_success 'unmerged files exist after conflict' '
	cd repo &&
	git ls-files -u >unmerged &&
	test -s unmerged
'

test_done
