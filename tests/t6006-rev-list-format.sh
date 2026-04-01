#!/bin/sh
# Ported subset from git/t/t6006-rev-list-format.sh.

test_description='git rev-list format output'

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

test_expect_success 'setup repository with two commits' '
	gust init repo &&
	cd repo &&
	head1=$(doit 1 "added foo") &&
	head2=$(doit 2 "changed foo" "$head1") &&
	git update-ref refs/heads/main "$head2" &&
	echo "$head1" >head1 &&
	echo "$head2" >head2
'

test_expect_success '--format=%s includes commit headers' '
	cd repo &&
	head1=$(cat head1) &&
	head2=$(cat head2) &&
	cat >expect <<-EOF &&
	commit $head2
	changed foo
	commit $head1
	added foo
	EOF
	git rev-list --format=%s refs/heads/main >actual &&
	test_cmp expect actual
'

test_expect_success '--format supports %H and %h' '
	cd repo &&
	head2=$(cat head2) &&
	short2=$(git rev-parse --short=4 "$head2") &&
	cat >expect <<-EOF &&
	commit $head2
	$head2 $short2
	EOF
	git rev-list --abbrev=4 --max-count=1 --format="%H %h" refs/heads/main >actual &&
	test_cmp expect actual
'

test_expect_success '--quiet suppresses output' '
	cd repo &&
	git rev-list --quiet refs/heads/main >actual &&
	test_path_is_file actual &&
	lines=$(wc -c <actual | tr -d " ") &&
	test "$lines" = "0"
'

test_done
