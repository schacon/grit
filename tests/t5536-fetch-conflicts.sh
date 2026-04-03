#!/bin/sh

test_description='fetch handles conflicting refspecs correctly'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

D=$(pwd)

setup_repository () {
	git init "$1" && (
		cd "$1" &&
		git config remote.origin.url "$D" &&
		shift &&
		for refspec in "$@"
		do
			git config --add remote.origin.fetch "$refspec"
		done
	)
}

test_expect_success 'setup' '
	git init &&
	git commit --allow-empty -m "Initial" &&
	git branch branch1 &&
	git tag tag1 &&
	git commit --allow-empty -m "First" &&
	git branch branch2 &&
	git tag tag2
'

test_expect_success 'fetch with no conflict' '
	setup_repository ok "+refs/heads/*:refs/remotes/origin/*" && (
		cd ok &&
		git fetch origin
	)
'

test_expect_success 'fetch duplicate: config vs. config' '
	setup_repository dcc \
		"+refs/heads/*:refs/remotes/origin/*" \
		"+refs/heads/branch1:refs/remotes/origin/branch1" && (
		cd dcc &&
		git fetch origin
	)
'

test_done
