#!/bin/sh

test_description='"git push --set-upstream" basic tests.'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

check_config () {
	printf "%s\n" "$2" "$3" >"expect.$1" &&
	{
		git config "branch.$1.remote" && git config "branch.$1.merge"
	} >"actual.$1" &&
	test_cmp "expect.$1" "actual.$1"
}

check_config_missing () {
	test_expect_code 1 git config "branch.$1.remote" &&
	test_expect_code 1 git config "branch.$1.merge"
}

clear_config () {
	for branch in "$@"; do
		test_might_fail git config --unset-all "branch.$branch.remote"
		test_might_fail git config --unset-all "branch.$branch.merge"
	done
}

ensure_fresh_upstream () {
	rm -rf parent && git init --bare parent
}

test_expect_success 'setup bare parent' '
	git init &&
	ensure_fresh_upstream &&
	git remote add upstream parent
'

test_expect_success 'setup commit on master and other' '
	test_commit one &&
	git push upstream master &&
	git checkout -b other &&
	test_commit two &&
	git push upstream other
'

test_expect_success 'push --set-upstream upstream master sets branch master but not other' '
	clear_config master other &&
	git checkout master &&
	git push --set-upstream upstream master &&
	check_config master upstream refs/heads/master &&
	check_config_missing other
'

test_expect_success 'push --set-upstream upstream other sets branch other' '
	clear_config master other &&
	git checkout other &&
	git push --set-upstream upstream other &&
	check_config other upstream refs/heads/other &&
	check_config_missing master
'

test_done
