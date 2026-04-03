#!/bin/sh

test_description='reflog walk shows repeated commits again'

. ./test-lib.sh

test_expect_success 'setup commits' '
	git init -q &&
	git config user.name "Test" &&
	git config user.email "t@t" &&
	echo content >file &&
	git add file &&
	git commit -m one &&
	git tag one &&
	echo more >>file &&
	git add file &&
	git commit -m two &&
	git tag two
'

test_expect_success 'setup reflog with alternating commits' '
	git checkout -b topic &&
	git reset one &&
	git reset two &&
	git reset one &&
	git reset two
'

test_expect_success 'reflog shows all entries with alternating resets' '
	git reflog show topic >actual &&
	grep "reset: moving to two" actual &&
	grep "reset: moving to one" actual &&
	test_line_count -ge 4 actual
'

test_expect_success 'reflog entries alternate correctly' '
	git reflog show topic >actual &&
	head -1 actual | grep "reset: moving to two" &&
	head -2 actual | tail -1 | grep "reset: moving to one" &&
	head -3 actual | tail -1 | grep "reset: moving to two" &&
	head -4 actual | tail -1 | grep "reset: moving to one"
'

test_done
