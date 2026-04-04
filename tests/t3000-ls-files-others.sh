#!/bin/sh
#
# Copyright (c) 2005 Junio C Hamano
#

test_description='basic tests for ls-files --others'

. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_success 'setup' '
	date >path0 &&
	date >path1 &&
	mkdir path2 path3 &&
	date >path2/file2 &&
	date >path2-junk &&
	date >path3/file3 &&
	date >path3-junk &&
	git update-index --add path3-junk path3/file3
'

test_expect_success 'setup: expected output' '
	cat >expected1 <<-\EOF &&
	.bin/git
	.bin/grit
	.bin/scalar
	expected1
	expected2
	output
	path0
	path1
	path2-junk
	path2/file2
	EOF

	cp expected1 expected2
'

test_expect_success 'ls-files --others' '
	git ls-files --others >output &&
	test_cmp expected1 output
'

test_expect_success 'ls-files --others handles non-submodule .git' '
	mkdir not-a-submodule &&
	echo foo >not-a-submodule/.git &&
	git ls-files -o >output &&
	test_cmp expected1 output
'

test_done
