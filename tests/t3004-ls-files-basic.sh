#!/bin/sh
# Ported from git/t/t3004-ls-files-basic.sh (harness-compatible subset).

test_description='grit ls-files basic'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repository' '
	grit init repo &&
	cd repo
'

test_expect_success 'ls-files in empty repository' '
	cd repo &&
	: >expect &&
	grit ls-files >actual &&
	test_cmp expect actual
'

test_expect_success 'ls-files with nonexistent path is empty' '
	cd repo &&
	: >expect &&
	grit ls-files doesnotexist >actual &&
	test_cmp expect actual
'

test_expect_success 'ls-files with nonsense option' '
	cd repo &&
	test_must_fail grit ls-files --nonsense 2>actual
'

test_expect_success 'ls-files lists tracked paths after update-index --add' '
	cd repo &&
	echo one >one &&
	echo two >two &&
	grit update-index --add one two &&
	cat >expect <<-\EOF &&
	one
	two
	EOF
	grit ls-files >actual &&
	test_cmp expect actual
'

test_expect_success 'ls-files --stage shows mode oid stage path' '
	cd repo &&
	one_oid=$(grit hash-object -w one) &&
	two_oid=$(grit hash-object -w two) &&
	cat >expect <<-EOF &&
	100644 $one_oid 0	one
	100644 $two_oid 0	two
	EOF
	grit ls-files --stage one two >actual &&
	test_cmp expect actual
'

test_expect_success 'ls-files -s shows mode oid stage for all tracked' '
	cd repo &&
	grit ls-files -s >actual &&
	test_line_count = 2 actual &&
	grep "^100644" actual
'

test_done
