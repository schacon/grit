#!/bin/sh

test_description='Test git pack-redundant'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	echo "content A" >fileA &&
	git add fileA &&
	test_tick &&
	git commit -m "A" &&
	echo "content B" >fileB &&
	git add fileB &&
	test_tick &&
	git commit -m "B" &&
	echo "content C" >fileC &&
	git add fileC &&
	test_tick &&
	git commit -m "C" &&
	git repack -ad
'

test_expect_success 'pack-redundant --all with no redundancy' '
	git pack-redundant --all >redundant &&
	test_must_be_empty redundant
'

test_expect_success 'create redundant pack' '
	git pack-objects .git/objects/pack/dup-pack --all &&
	ls .git/objects/pack/*.pack >packs &&
	test_line_count -ge 2 packs
'

test_expect_success 'pack-redundant detects redundant packs' '
	git pack-redundant --all >redundant &&
	test_file_not_empty redundant
'

test_done
