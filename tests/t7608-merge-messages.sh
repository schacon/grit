#!/bin/sh

test_description='test auto-generated merge messages'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

check_oneline() {
	echo "$1" | sed "s/Q/'/g" >expect &&
	git log -n 1 --format="%s" >actual &&
	test_cmp expect actual
}

test_expect_success 'setup' '
	git init merge-msgs &&
	cd merge-msgs &&
	echo base >file &&
	git add file &&
	test_tick &&
	git commit -m "main-1" &&
	git tag main-1
'

test_expect_success 'merge local branch' '
	cd merge-msgs &&
	git checkout -b local-branch &&
	echo branch >branch-file &&
	git add branch-file &&
	test_tick &&
	git commit -m "branch-1" &&
	git tag branch-1 &&
	git checkout master &&
	echo main2 >main-file &&
	git add main-file &&
	test_tick &&
	git commit -m "main-2" &&
	git tag main-2 &&
	git merge local-branch &&
	check_oneline "Merge branch Qlocal-branchQ"
'

test_expect_success 'merge with custom message overrides' '
	cd merge-msgs &&
	git checkout -b custom-branch main-1 &&
	echo custom >custom-file &&
	git add custom-file &&
	test_tick &&
	git commit -m "custom" &&
	git checkout master &&
	git merge -m "my custom merge" custom-branch &&
	check_oneline "my custom merge"
'

test_done
