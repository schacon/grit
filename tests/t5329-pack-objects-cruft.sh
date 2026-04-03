#!/bin/sh

test_description='pack-objects cruft and unreachable object handling'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	test_commit reachable &&
	git repack -ad
'

test_expect_success 'create unreachable objects' '
	for i in 1 2 3
	do
		echo "unreachable $i" | git hash-object -w --stdin || return 1
	done
'

test_expect_success 'pack-objects --all packs reachable objects' '
	git pack-objects --all testpack &&
	git verify-pack testpack-*.pack
'

test_expect_success 'prune removes unreachable loose objects' '
	git prune --expire=now &&
	git count-objects >count &&
	test_grep "0 objects" count
'

test_done
