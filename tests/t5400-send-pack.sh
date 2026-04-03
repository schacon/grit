#!/bin/sh

test_description='See why rewinding head breaks send-pack'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

cnt=64
test_expect_success setup '
	git init &&
	test_tick &&
	mkdir mozart mozart/is &&
	echo "Commit #0" >mozart/is/pink &&
	git update-index --add mozart/is/pink &&
	tree=$(git write-tree) &&
	commit=$(echo "Commit #0" | git commit-tree $tree) &&
	zero=$commit &&
	parent=$zero &&
	i=0 &&
	while test $i -le $cnt
	do
		i=$(($i+1)) &&
		test_tick &&
		echo "Commit #$i" >mozart/is/pink &&
		git update-index --add mozart/is/pink &&
		tree=$(git write-tree) &&
		commit=$(echo "Commit #$i" |
			 git commit-tree $tree -p $parent) &&
		git update-ref refs/tags/commit$i $commit &&
		parent=$commit || return 1
	done &&
	git update-ref HEAD "$commit" &&
	git clone ./. victim &&
	( cd victim && git config receive.denyCurrentBranch warn && git log --oneline | head -5 ) &&
	git update-ref HEAD "$zero" &&
	parent=$zero &&
	i=0 &&
	while test $i -le $cnt
	do
		i=$(($i+1)) &&
		test_tick &&
		echo "Rebase #$i" >mozart/is/pink &&
		git update-index --add mozart/is/pink &&
		tree=$(git write-tree) &&
		commit=$(echo "Rebase #$i" | git commit-tree $tree -p $parent) &&
		git update-ref refs/tags/rebase$i $commit &&
		parent=$commit || return 1
	done &&
	git update-ref HEAD "$commit"
'

test_expect_success 'pack the source repository' '
	git repack -a -d &&
	git prune
'

test_expect_success 'pack the destination repository' '
	(
		cd victim &&
		git repack -a -d &&
		git prune
	)
'

test_expect_success 'refuse pushing rewound head without --force' '
	pushed_head=$(git rev-parse --verify master) &&
	victim_orig=$(cd victim && git rev-parse --verify master) &&
	test_must_fail git send-pack ./victim master &&
	victim_head=$(cd victim && git rev-parse --verify master) &&
	test "$victim_head" = "$victim_orig" &&
	# this should update
	git send-pack --force ./victim master &&
	victim_head=$(cd victim && git rev-parse --verify master) &&
	test "$victim_head" = "$pushed_head"
'

test_done
