#!/bin/sh
# Ported from git/t/t5565-push-multiple.sh
# Tests push to group (multiple pushurls)

test_description='push to group'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	git init -q &&
	for i in 1 2 3
	do
		git init dest-$i &&
		git -C dest-$i symbolic-ref HEAD refs/heads/not-a-branch ||
		return 1
	done &&
	test_tick &&
	git commit --allow-empty -m "initial" &&
	git config --add remote.them.pushurl "$(pwd)/dest-1" &&
	git config --add remote.them.pushurl "$(pwd)/dest-2" &&
	git config --add remote.them.pushurl "$(pwd)/dest-3" &&
	git config --add remote.them.push "+refs/heads/*:refs/heads/*"
'

# grit does not yet support multi-pushurl remotes
test_expect_failure 'push to group' '
	git push them &&
	j= &&
	for i in 1 2 3
	do
		git -C dest-$i for-each-ref >actual-$i &&
		if test -n "$j"
		then
			test_cmp actual-$j actual-$i
		else
			cat actual-$i
		fi &&
		j=$i ||
		return 1
	done
'

test_done
