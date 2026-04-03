#!/bin/sh
# Ported from upstream git t7811-grep-open.sh

test_description='grep open (grep functionality tests)'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init grep-open &&
	cd grep-open &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo "hello world" >file1 &&
	echo "foo bar" >file2 &&
	echo "hello foo" >file3 &&
	git add . &&
	test_tick &&
	git commit -m initial
'

test_expect_success 'grep finds pattern in working tree' '
	cd grep-open &&
	git grep hello >actual &&
	test $(wc -l <actual) -eq 2
'

test_expect_success 'grep -l shows filenames only' '
	cd grep-open &&
	git grep -l hello >actual &&
	test $(wc -l <actual) -eq 2
'

test_expect_success 'grep in committed tree' '
	cd grep-open &&
	git grep hello HEAD >actual &&
	test $(wc -l <actual) -eq 2
'

test_expect_success 'grep with pathspec' '
	cd grep-open &&
	git grep hello -- file1 >actual &&
	test $(wc -l <actual) -eq 1
'

test_expect_success 'grep -n shows line numbers' '
	cd grep-open &&
	git grep -n hello >actual &&
	grep ":1:" actual
'

test_expect_success 'grep -c shows counts' '
	cd grep-open &&
	git grep -c hello >actual &&
	test $(wc -l <actual) -eq 2
'

test_expect_success 'grep -w matches whole words' '
	cd grep-open &&
	git grep -w foo >actual &&
	test $(wc -l <actual) -eq 2
'

test_done
