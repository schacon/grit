#!/bin/sh
#
# Ported from git/t/t0041-usage.sh
# Subset that works with grit's current feature set.

test_description='Test commands behavior when given invalid argument value'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q
'

test_expect_success 'setup' '
	test_commit "v1.0"
'

test_expect_success 'tag --contains <existent_tag>' '
	git tag --contains "v1.0" >actual 2>actual.err &&
	grep "v1.0" actual &&
	test_line_count = 0 actual.err
'

test_expect_success 'tag --contains <inexistent_tag>' '
	test_must_fail git tag --contains "notag" >actual 2>actual.err &&
	test_line_count = 0 actual &&
	test_grep "error" actual.err
'

test_expect_success 'branch --contains <existent_commit>' '
	git branch --contains "master" >actual 2>actual.err &&
	test_grep "master" actual &&
	test_line_count = 0 actual.err
'

test_expect_success 'branch --no-contains <inexistent_commit>' '
	test_must_fail git branch --no-contains "nocommit" >actual 2>actual.err &&
	test_line_count = 0 actual &&
	test_grep "error" actual.err
'

test_expect_success 'branch --no-contains <existent_commit>' '
	git branch --no-contains "master" >actual 2>actual.err &&
	test_line_count = 0 actual &&
	test_line_count = 0 actual.err
'

test_expect_success 'branch --contains <inexistent_commit>' '
	test_must_fail git branch --no-contains "nocommit" >actual 2>actual.err &&
	test_line_count = 0 actual &&
	test_grep "error" actual.err
'

test_expect_success 'for-each-ref --no-contains <existent_object>' '
	git for-each-ref --no-contains "master" >actual 2>actual.err &&
	test_line_count = 0 actual &&
	test_line_count = 0 actual.err
'

test_done
