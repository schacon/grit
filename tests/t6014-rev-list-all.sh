#!/bin/sh
# Ported subset from git/t/t6014-rev-list-all.sh.

test_description='rev-list --all includes detached HEADs'

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

test_expect_success 'setup detached HEAD' '
	grit init repo &&
	cd repo &&
	one=$(doit 1 one) &&
	two=$(doit 2 two "$one") &&
	detached=$(doit 3 detached "$one") &&
	git update-ref refs/heads/main "$two" &&
	echo "$detached" >.git/HEAD
'

test_expect_success '--all includes detached HEAD commits' '
	cd repo &&
	lines=$(git rev-list --all | wc -l | tr -d " ") &&
	test "$lines" = "3"
'

test_done
