#!/bin/sh

test_description='git ls-files --others shows untracked files'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	mkdir -p one/two &&
	for i in 1 2 3 4 5 6 7 8; do
		>a.$i &&
		>one/a.$i &&
		>one/two/a.$i
	done
'

test_expect_success 'ls-files --others shows all untracked files' '
	git ls-files --others >output &&
	test_line_count -ge 24 output
'

test_expect_success 'ls-files --others shows files in subdirectories' '
	git ls-files --others >output &&
	grep "^one/a\.1$" output &&
	grep "^one/two/a\.1$" output
'

test_expect_success 'adding files removes them from --others listing' '
	git add a.1 a.2 &&
	git ls-files --others >output &&
	! grep "^a\.1$" output &&
	! grep "^a\.2$" output &&
	grep "^a\.3$" output
'

test_expect_success 'ls-files with no flags shows tracked files' '
	git ls-files >output &&
	grep "^a\.1$" output &&
	grep "^a\.2$" output &&
	! grep "^a\.3$" output
'

test_done
