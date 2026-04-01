#!/bin/sh
# Ported subset from git/t/t6005-rev-list-count.sh.

test_description='git rev-list --max-count and --skip'

. ./test-lib.sh

GIT_COMMITTER_EMAIL=git@comm.iter.xz
GIT_COMMITTER_NAME='C O Mmiter'
GIT_AUTHOR_NAME='A U Thor'
GIT_AUTHOR_EMAIL=git@au.thor.xz
export GIT_COMMITTER_EMAIL GIT_COMMITTER_NAME GIT_AUTHOR_NAME GIT_AUTHOR_EMAIL

M=1130000000
Z=+0000
export M Z

doit () {
	OFFSET=$1 &&
	NAME=$2 &&
	shift 2 &&
	PARENTS= &&
	for P
	do
		PARENTS="$PARENTS -p $P"
	done &&
	GIT_COMMITTER_DATE="$(($M + $OFFSET)) $Z" &&
	GIT_AUTHOR_DATE="$GIT_COMMITTER_DATE" &&
	export GIT_COMMITTER_DATE GIT_AUTHOR_DATE &&
	commit=$(echo "$NAME" | git commit-tree "$(git write-tree)" $PARENTS) &&
	echo "$commit"
}

test_expect_success 'setup linear history' '
	grit init repo &&
	cd repo &&
	c1=$(doit 1 one) &&
	c2=$(doit 2 two "$c1") &&
	c3=$(doit 3 three "$c2") &&
	c4=$(doit 4 four "$c3") &&
	c5=$(doit 5 five "$c4") &&
	git update-ref refs/heads/main "$c5"
'

test_expect_success 'plain count' '
	cd repo &&
	lines=$(git rev-list refs/heads/main | wc -l | tr -d " ") &&
	test "$lines" = "5"
'

test_expect_success '--max-count forms' '
	cd repo &&
	test_must_fail git rev-list --max-count=bogus refs/heads/main &&
	test_must_fail git rev-list -n bogus refs/heads/main &&
	lines=$(git rev-list --max-count=3 refs/heads/main | wc -l | tr -d " ") &&
	test "$lines" = "3" &&
	lines=$(git rev-list -1 refs/heads/main | wc -l | tr -d " ") &&
	test "$lines" = "1" &&
	lines=$(git rev-list -n 2 refs/heads/main | wc -l | tr -d " ") &&
	test "$lines" = "2"
'

test_expect_success '--skip with --max-count' '
	cd repo &&
	lines=$(git rev-list --skip=3 refs/heads/main | wc -l | tr -d " ") &&
	test "$lines" = "2" &&
	lines=$(git rev-list --skip=3 --max-count=1 refs/heads/main | wc -l | tr -d " ") &&
	test "$lines" = "1" &&
	lines=$(git rev-list --skip=3 --max-count=10 refs/heads/main | wc -l | tr -d " ") &&
	test "$lines" = "2"
'

test_expect_success '--count matches listed commits' '
	cd repo &&
	count=$(git rev-list --count --skip=1 --max-count=2 refs/heads/main) &&
	lines=$(git rev-list --skip=1 --max-count=2 refs/heads/main | wc -l | tr -d " ") &&
	test "$count" = "$lines"
'

test_done
