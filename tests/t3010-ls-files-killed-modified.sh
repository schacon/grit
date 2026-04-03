#!/bin/sh

test_description='git ls-files basic flag tests'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	echo content >tracked &&
	echo other >tracked2 &&
	git add tracked tracked2 &&
	git commit -m "initial" &&
	echo untracked >untracked_file
'

test_expect_success 'ls-files --cached shows tracked files' '
	git ls-files --cached >actual &&
	cat >expect <<-\EOF &&
	tracked
	tracked2
	EOF
	test_cmp expect actual
'

test_expect_success 'ls-files --others shows untracked' '
	git ls-files --others >actual &&
	grep untracked_file actual
'

test_expect_success 'ls-files -s shows staged info' '
	git ls-files -s >actual &&
	test_line_count = 2 actual &&
	grep "tracked" actual
'

test_done
