#!/bin/sh
#
# Copyright (c) 2005 Junio C Hamano
#
# Ported from git/t/t3002-ls-files-dashpath.sh

test_description='git ls-files test (-- to terminate the path list).

This test runs git ls-files --others with the following on the
filesystem.

    path0       - a file
    -foo	- a file with a funny name.
    --		- another file with a funny name.
'

. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	echo frotz >path0 &&
	echo frotz >./-foo &&
	echo frotz >./--
'

test_expect_success 'git ls-files without path restriction.' '
	cd repo &&
	git ls-files --others >output &&
	cat >expect <<-\EOF &&
	--
	-foo
	output
	path0
	EOF
	test_cmp output expect
'

test_expect_success 'git ls-files with path restriction.' '
	cd repo &&
	rm -f expect &&
	git ls-files --others path0 >output &&
	cat >expect <<-\EOF &&
	path0
	EOF
	test_cmp output expect
'

test_expect_success 'git ls-files with path restriction with --.' '
	cd repo &&
	rm -f expect &&
	git ls-files --others -- path0 >output &&
	cat >expect <<-\EOF &&
	path0
	EOF
	test_cmp output expect
'

test_expect_success 'git ls-files with path restriction with -- --.' '
	cd repo &&
	rm -f expect &&
	git ls-files --others -- -- >output &&
	cat >expect <<-\EOF &&
	--
	EOF
	test_cmp output expect
'

test_expect_success 'git ls-files with no path restriction.' '
	cd repo &&
	rm -f expect &&
	git ls-files --others -- >output &&
	cat >expect <<-\EOF &&
	--
	-foo
	output
	path0
	EOF
	test_cmp output expect
'

test_done
