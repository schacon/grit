#!/bin/sh
# Ported subset from git/t/t6017-rev-list-stdin.sh.

test_description='rev-list reads revisions from --stdin'

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

test_expect_success 'setup history' '
	gust init repo &&
	cd repo &&
	c1=$(doit 1 one) &&
	c2=$(doit 2 two "$c1") &&
	c3=$(doit 3 three "$c2") &&
	side=$(doit 4 side "$c2") &&
	git update-ref refs/heads/main "$c3" &&
	git update-ref refs/heads/side "$side"
'

test_expect_success 'stdin and command-line revisions agree' '
	cd repo &&
	printf "%s\n" side ^main >input &&
	git rev-list side ^main >expect &&
	git rev-list --stdin <input >actual &&
	test_cmp expect actual
'

test_expect_success '--not from stdin only affects stdin revisions' '
	cd repo &&
	cat >input <<-EOF &&
	--not
	EOF
	git rev-list main >expect &&
	git rev-list main --stdin <input >actual &&
	test_cmp expect actual
'

test_expect_success '--all accepted from stdin' '
	cd repo &&
	printf "%s\n" --all >input &&
	git rev-list --all >expect &&
	git rev-list --stdin <input >actual &&
	test_cmp expect actual
'

test_done
