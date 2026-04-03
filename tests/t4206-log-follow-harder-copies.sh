#!/bin/sh
#
# Copyright (c) 2010 Bo Yang
#

test_description='git log with various format options'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "A U Thor" &&
	git config user.email "author@example.com" &&

	echo "Line 1" >path0 &&
	echo "Line 2" >>path0 &&
	echo "Line 3" >>path0 &&
	git add path0 &&
	test_tick &&
	git commit -q -m "Add path0" &&

	echo "New line 1" >path0 &&
	echo "New line 2" >>path0 &&
	echo "New line 3" >>path0 &&
	git add path0 &&
	test_tick &&
	git commit -q -m "Change path0"
'

test_expect_success 'log --format=%H shows full hashes' '
	git log --format="%H" >actual &&
	test_line_count = 2 actual &&
	grep "^[0-9a-f]\{40\}$" actual >matches &&
	test_line_count = 2 matches
'

test_expect_success 'log --format=%s shows subjects' '
	git log --format="%s" >actual &&
	cat >expect <<-\EOF &&
	Change path0
	Add path0
	EOF
	test_cmp expect actual
'

test_expect_success 'log --format=%an shows author names' '
	git log --format="%an" >actual &&
	cat >expect <<-\EOF &&
	A U Thor
	A U Thor
	EOF
	test_cmp expect actual
'

test_expect_success 'log --format=%ae shows author emails' '
	git log --format="%ae" >actual &&
	cat >expect <<-\EOF &&
	author@example.com
	author@example.com
	EOF
	test_cmp expect actual
'

test_expect_success 'log --reverse shows oldest first' '
	git log --reverse --format="%s" >actual &&
	cat >expect <<-\EOF &&
	Add path0
	Change path0
	EOF
	test_cmp expect actual
'

test_expect_success 'log with revision argument' '
	head=$(git rev-parse HEAD) &&
	git log --format="%s" $head >actual &&
	test_line_count = 2 actual
'

test_done
