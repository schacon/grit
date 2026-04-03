#!/bin/sh

test_description='Test commit and tag messages using CRLF'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup initial commit' '
	git init repo &&
	cd repo &&
	test_commit initial
'

test_expect_success 'commit with CRLF in message body' '
	cd repo &&
	printf "Subject line\r\n\r\nBody line one\r\nBody line two\r\n" >msg &&
	test_tick &&
	git commit --allow-empty -F msg &&
	git log --max-count=1 --format="%s" >actual &&
	echo "Subject line" >expect &&
	test_cmp expect actual
'

test_expect_success 'log shows correct subject with CRLF' '
	cd repo &&
	git log --oneline --max-count=1 >actual &&
	grep "Subject line" actual
'

test_done
