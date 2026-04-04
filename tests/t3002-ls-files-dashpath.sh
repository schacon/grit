#!/bin/sh
#
# Copyright (c) 2005 Junio C Hamano
#

test_description='git ls-files test (-- to terminate the path list).'

. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_success 'setup' '
	echo frotz >path0 &&
	echo frotz >./-foo &&
	echo frotz >./--
'

test_expect_success 'git ls-files without path restriction.' '
	test_when_finished "rm -f expect" &&
	git ls-files --others >output &&
	cat >expect <<-\EOF &&
	--
	-foo
	.bin/git
	.bin/grit
	.bin/scalar
	output
	path0
	EOF
	test_cmp output expect
'

test_expect_success 'git ls-files with path restriction.' '
	test_when_finished "rm -f expect" &&
	git ls-files --others path0 >output &&
	cat >expect <<-\EOF &&
	path0
	EOF
	test_cmp output expect
'

test_expect_success 'git ls-files with path restriction with --.' '
	test_when_finished "rm -f expect" &&
	git ls-files --others -- path0 >output &&
	cat >expect <<-\EOF &&
	path0
	EOF
	test_cmp output expect
'

test_expect_success 'git ls-files with path restriction with -- --.' '
	test_when_finished "rm -f expect" &&
	git ls-files --others -- -- >output &&
	cat >expect <<-\EOF &&
	--
	EOF
	test_cmp output expect
'

test_expect_success 'git ls-files with no path restriction.' '
	test_when_finished "rm -f expect" &&
	git ls-files --others -- >output &&
	cat >expect <<-\EOF &&
	--
	-foo
	.bin/git
	.bin/grit
	.bin/scalar
	output
	path0
	EOF
	test_cmp output expect
'

test_done
