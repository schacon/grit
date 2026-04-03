#!/bin/sh

test_description='pack-objects delta depth handling'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	echo "initial content" >file &&
	git add file &&
	test_tick &&
	git commit -m "initial"
'

test_expect_success 'create series of modifications' '
	for i in 1 2 3 4 5
	do
		echo "modification $i" >>file &&
		git add file &&
		test_tick &&
		git commit -m "change $i" || return 1
	done
'

test_expect_success 'repack and verify' '
	git repack -ad &&
	git verify-pack .git/objects/pack/pack-*.pack
'

test_expect_success 'verify-pack -v shows chain stats' '
	git verify-pack -v .git/objects/pack/pack-*.pack >output &&
	test_grep "chain length" output
'

test_expect_success 'pack-objects --all produces valid pack' '
	git pack-objects --all testpack &&
	git verify-pack testpack-*.pack
'

test_done
