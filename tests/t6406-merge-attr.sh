#!/bin/sh

test_description='merge with custom attributes'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test" &&
	test_write_lines 1 2 3 4 5 >text &&
	git add text &&
	git commit -m base &&
	git branch side &&

	sed "s/2/two/" <text >tmp && mv tmp text &&
	git add text &&
	git commit -m main &&

	git checkout side &&
	sed "s/4/four/" <text >tmp && mv tmp text &&
	git add text &&
	git commit -m side
'

test_expect_success 'default merge resolves cleanly' '
	cd repo &&
	git checkout main &&
	git merge side &&
	grep two text &&
	grep four text
'

test_done
