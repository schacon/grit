#!/bin/sh

test_description='Test ls-files with various options'

. ./test-lib.sh

test_expect_success 'setup directory structure' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo a >a &&
	mkdir b &&
	echo b >b/b &&
	git add a b &&
	git commit -m "add a and b"
'

test_expect_success 'ls-files correctly outputs files' '
	cat >expect <<-\EOF &&
	a
	b/b
	EOF
	git ls-files >actual &&
	test_cmp expect actual
'

test_expect_success 'ls-files --stage shows mode and sha' '
	git ls-files --stage >out &&
	test_line_count = 2 out &&
	grep "^100644" out &&
	grep "a$" out &&
	grep "b/b$" out
'

test_expect_success 'ls-files with pathspec limits output' '
	git ls-files b/ >actual &&
	echo "b/b" >expect &&
	test_cmp expect actual
'

test_done
