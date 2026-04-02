#!/bin/sh
# Tests for ls-files functionality.
# Upstream git t3060 covers --with-tree; grit doesn't support that flag yet.
# We test ls-files --cached, --stage, -z, pathspecs, --error-unmatch,
# --deduplicate, and interaction with index changes.

test_description='ls-files cached, stage, pathspecs, and flags'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup ls-files repo' '
	git init lsf-repo &&
	cd lsf-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo "file a" >a.txt &&
	echo "file b" >b.txt &&
	mkdir sub &&
	echo "file c" >sub/c.txt &&
	git add . &&
	test_tick &&
	git commit -m "initial"
'

test_expect_success 'ls-files shows all tracked files' '
	cd lsf-repo &&
	git ls-files >actual &&
	cat >expect <<-\EOF &&
	a.txt
	b.txt
	sub/c.txt
	EOF
	test_cmp expect actual
'

test_expect_success 'ls-files --cached is the default' '
	cd lsf-repo &&
	git ls-files >default_out &&
	git ls-files --cached >cached_out &&
	test_cmp default_out cached_out
'

test_expect_success 'ls-files --stage shows object info' '
	cd lsf-repo &&
	git ls-files --stage >actual &&
	# Format: mode hash stage<tab>filename
	grep "^100644 " actual &&
	grep "	a.txt" actual &&
	grep "	b.txt" actual &&
	grep "	sub/c.txt" actual
'

test_expect_success 'ls-files --stage shows correct mode' '
	cd lsf-repo &&
	git ls-files --stage >actual &&
	# All regular files should be 100644
	while IFS= read -r line; do
		mode=$(echo "$line" | cut -d" " -f1)
		test "$mode" = "100644" || return 1
	done <actual
'

test_expect_success 'ls-files --stage shows stage number 0' '
	cd lsf-repo &&
	git ls-files --stage >actual &&
	# Normal files have stage 0
	grep "0	a.txt" actual
'

test_expect_success 'ls-files with pathspec restricts output' '
	cd lsf-repo &&
	git ls-files sub/ >actual &&
	echo "sub/c.txt" >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files with file pathspec' '
	cd lsf-repo &&
	git ls-files a.txt >actual &&
	echo "a.txt" >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files with multiple pathspecs' '
	cd lsf-repo &&
	git ls-files a.txt b.txt >actual &&
	cat >expect <<-\EOF &&
	a.txt
	b.txt
	EOF
	test_cmp expect actual
'

test_expect_success 'ls-files -z uses NUL termination' '
	cd lsf-repo &&
	git ls-files -z >actual_raw &&
	# Should contain NUL bytes
	tr "\0" "\n" <actual_raw >actual &&
	grep "a.txt" actual &&
	grep "b.txt" actual
'

test_expect_success 'ls-files --error-unmatch with tracked file succeeds' '
	cd lsf-repo &&
	git ls-files --error-unmatch a.txt >actual &&
	echo "a.txt" >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files --error-unmatch with untracked file fails' '
	cd lsf-repo &&
	test_must_fail git ls-files --error-unmatch nonexist.txt
'

test_expect_success 'ls-files --deduplicate shows unique entries' '
	cd lsf-repo &&
	git ls-files --deduplicate >actual &&
	test_line_count = 3 actual
'

test_expect_success 'ls-files after adding a new file' '
	cd lsf-repo &&
	echo "file d" >d.txt &&
	git add d.txt &&
	git ls-files >actual &&
	grep "d.txt" actual &&
	test_line_count = 4 actual
'

test_expect_success 'ls-files after committing new file' '
	cd lsf-repo &&
	test_tick &&
	git commit -m "add d" &&
	git ls-files >actual &&
	test_line_count = 4 actual
'

test_expect_success 'ls-files after removing a file from index' '
	cd lsf-repo &&
	git rm d.txt &&
	git ls-files >actual &&
	! grep "d.txt" actual &&
	test_line_count = 3 actual
'

test_expect_success 'ls-files with nested directories' '
	cd lsf-repo &&
	test_tick &&
	git commit -m "rm d" &&
	mkdir -p deep/nested/dir &&
	echo "deep file" >deep/nested/dir/deep.txt &&
	git add deep/ &&
	test_tick &&
	git commit -m "add deep" &&
	git ls-files >actual &&
	grep "deep/nested/dir/deep.txt" actual
'

test_expect_success 'ls-files pathspec on nested dir' '
	cd lsf-repo &&
	git ls-files deep/ >actual &&
	echo "deep/nested/dir/deep.txt" >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files pathspec on intermediate dir' '
	cd lsf-repo &&
	git ls-files deep/nested/ >actual &&
	echo "deep/nested/dir/deep.txt" >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files output is sorted' '
	cd lsf-repo &&
	git ls-files >actual &&
	sort actual >sorted &&
	test_cmp sorted actual
'

test_expect_success 'ls-files --stage shows hash for each file' '
	cd lsf-repo &&
	git ls-files --stage >actual &&
	while IFS= read -r line; do
		hash=$(echo "$line" | awk "{print \$2}")
		# Hash should be 40 hex chars
		test $(echo "$hash" | wc -c) -ge 40 || return 1
	done <actual
'

test_expect_success 'ls-files --stage hash matches cat-file' '
	cd lsf-repo &&
	hash=$(git ls-files --stage a.txt | awk "{print \$2}") &&
	git cat-file -p $hash >actual &&
	echo "file a" >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files with file in subdirectory pathspec' '
	cd lsf-repo &&
	git ls-files sub/c.txt >actual &&
	echo "sub/c.txt" >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files with nonexistent pathspec shows nothing' '
	cd lsf-repo &&
	git ls-files nosuchdir/ >actual &&
	test_must_be_empty actual
'

test_expect_success 'ls-files -z with pathspec' '
	cd lsf-repo &&
	git ls-files -z sub/ >actual_raw &&
	tr "\0" "\n" <actual_raw >actual &&
	grep "sub/c.txt" actual
'

test_expect_success 'ls-files --stage -z uses NUL termination' '
	cd lsf-repo &&
	git ls-files --stage -z >actual_raw &&
	tr "\0" "\n" <actual_raw >actual &&
	grep "100644" actual &&
	grep "a.txt" actual
'

test_done
