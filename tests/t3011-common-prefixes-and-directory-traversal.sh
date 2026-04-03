#!/bin/sh

test_description='directory traversal handling with ls-files'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	echo hello >tracked &&
	git add tracked &&
	git commit -m "initial" &&

	echo bar >untracked_file &&
	mkdir untracked_dir &&
	echo baz >untracked_dir/file
'

test_expect_success 'ls-files -o shows untracked files' '
	git ls-files -o >actual &&
	grep untracked_file actual &&
	grep untracked_dir/file actual
'

test_expect_success 'ls-files -o with pathspec restricts output' '
	git ls-files -o untracked_dir >actual &&
	echo untracked_dir/file >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files -o .git shows nothing' '
	git ls-files -o .git >actual &&
	test_must_be_empty actual
'

test_expect_success 'ls-files --cached only shows tracked' '
	git ls-files --cached >actual &&
	echo tracked >expect &&
	test_cmp expect actual
'

test_done
