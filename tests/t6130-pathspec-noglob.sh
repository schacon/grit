#!/bin/sh
# Tests for pathspec with literal matching (no glob interpretation).
# Verifies that :(literal) magic and related behaviors work correctly.

test_description='pathspec literal (noglob) matching'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ── Setup ────────────────────────────────────────────────────────────────────

test_expect_success 'setup repository with special-character filenames' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	mkdir -p dir sub/nested &&
	echo "normal" >file.txt &&
	echo "star" >"star*.txt" &&
	echo "question" >"question?.txt" &&
	echo "bracket" >"a[1].txt" &&
	echo "brace" >"b{x,y}.txt" &&
	echo "range" >"c[a-z].txt" &&
	echo "dir-star" >"dir/star*.txt" &&
	echo "dir-normal" >dir/normal.txt &&
	echo "nested" >sub/nested/file.txt &&
	echo "hash" >"hash#file.txt" &&
	echo "bang" >"bang!file.txt" &&
	echo "plus" >"plus+file.txt" &&
	echo "equals" >"equals=file.txt" &&
	echo "at" >"at@file.txt" &&
	echo "pct" >"percent%file.txt" &&
	echo "space" >"has space.txt" &&
	echo "multi" >"multi[*]?.txt" &&
	git add . &&
	git commit -m "initial with special chars"
'

# ── Basic literal matching ───────────────────────────────────────────────────

test_expect_success 'ls-files with bracket filename matches literally by default' '
	cd repo &&
	git ls-files "a[1].txt" >actual &&
	echo "a[1].txt" >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files with star in filename: glob vs literal ambiguity' '
	cd repo &&
	git ls-files "star*.txt" >actual &&
	echo "star*.txt" >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files question mark filename matches literally' '
	cd repo &&
	git ls-files "question?.txt" >actual &&
	echo "question?.txt" >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files brace filename matches literally' '
	cd repo &&
	git ls-files "b{x,y}.txt" >actual &&
	echo "b{x,y}.txt" >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files range-like bracket filename' '
	cd repo &&
	git ls-files "c[a-z].txt" >actual &&
	echo "c[a-z].txt" >expect &&
	test_cmp expect actual
'

# ── Files with spaces and special ASCII ──────────────────────────────────────

test_expect_success 'ls-files with space in filename' '
	cd repo &&
	git ls-files "has space.txt" >actual &&
	echo "has space.txt" >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files with hash in filename' '
	cd repo &&
	git ls-files "hash#file.txt" >actual &&
	echo "hash#file.txt" >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files with bang in filename' '
	cd repo &&
	git ls-files "bang!file.txt" >actual &&
	echo "bang!file.txt" >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files with plus in filename' '
	cd repo &&
	git ls-files "plus+file.txt" >actual &&
	echo "plus+file.txt" >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files with equals in filename' '
	cd repo &&
	git ls-files "equals=file.txt" >actual &&
	echo "equals=file.txt" >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files with at-sign in filename' '
	cd repo &&
	git ls-files "at@file.txt" >actual &&
	echo "at@file.txt" >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files with percent in filename' '
	cd repo &&
	git ls-files "percent%file.txt" >actual &&
	echo "percent%file.txt" >expect &&
	test_cmp expect actual
'

# ── Multi-glob character filename ────────────────────────────────────────────

test_expect_success 'ls-files with combined glob chars in filename' '
	cd repo &&
	git ls-files "multi[*]?.txt" >actual &&
	echo "multi[*]?.txt" >expect &&
	test_cmp expect actual
'

# ── Directory pathspec with special chars ────────────────────────────────────

test_expect_success 'ls-files with dir prefix containing star file' '
	cd repo &&
	git ls-files "dir/star*.txt" >actual &&
	echo "dir/star*.txt" >expect &&
	test_cmp expect actual
'

test_expect_success 'ls-files with directory prefix shows all under dir' '
	cd repo &&
	git ls-files "dir/" >actual &&
	test_line_count = 2 actual
'

test_expect_success 'ls-files with nested directory prefix' '
	cd repo &&
	git ls-files "sub/" >actual &&
	echo "sub/nested/file.txt" >expect &&
	test_cmp expect actual
'

# ── add with special filenames ───────────────────────────────────────────────

test_expect_success 'add file with brackets by name' '
	cd repo &&
	echo "updated" >"a[1].txt" &&
	git add "a[1].txt" &&
	git diff --cached --name-only >actual &&
	grep "a\[1\].txt" actual
'

test_expect_success 'add file with star by name' '
	cd repo &&
	git commit -m "mid" --allow-empty &&
	echo "updated-star" >"star*.txt" &&
	git add "star*.txt" &&
	git diff --cached --name-only >actual &&
	grep "star\*.txt" actual
'

# ── rm with special filenames ────────────────────────────────────────────────

test_expect_success 'rm can remove bracket-named file' '
	cd repo &&
	git commit -m "pre-rm" -a &&
	echo "rmme" >"rmtest[1].txt" &&
	git add "rmtest[1].txt" &&
	git commit -m "add rmtest" &&
	git rm "rmtest[1].txt" &&
	test_path_is_missing "rmtest[1].txt" &&
	git ls-files "rmtest[1].txt" >actual &&
	test_must_be_empty actual
'

# ── No false matches ────────────────────────────────────────────────────────

test_expect_success 'ls-files nonexistent literal returns empty' '
	cd repo &&
	git ls-files "no-such-file[x].txt" >actual &&
	test_must_be_empty actual
'

test_expect_success 'ls-files exact name does not match unrelated prefix' '
	cd repo &&
	git ls-files "xyz-nonexistent" >actual &&
	test_must_be_empty actual
'

test_expect_success 'ls-files exact name matches exactly' '
	cd repo &&
	git ls-files "file.txt" >actual &&
	echo "file.txt" >expect &&
	test_cmp expect actual
'

# ── Multiple pathspecs ──────────────────────────────────────────────────────

test_expect_success 'ls-files with multiple literal pathspecs' '
	cd repo &&
	git ls-files "file.txt" "a[1].txt" >actual &&
	test_line_count = 2 actual &&
	grep "file.txt" actual &&
	grep "a\[1\].txt" actual
'

test_expect_success 'ls-files with mix of dir and file pathspecs' '
	cd repo &&
	git ls-files "file.txt" "dir/" >actual &&
	test_line_count = 3 actual
'

# ── Commit with special-char file in message ────────────────────────────────

test_expect_success 'commit touching only bracket file' '
	cd repo &&
	echo "again" >>"a[1].txt" &&
	git add "a[1].txt" &&
	git commit -m "update bracket file" &&
	git log -n1 --format="%s" >actual &&
	echo "update bracket file" >expect &&
	test_cmp expect actual
'

test_expect_success 'show commit diff includes bracket filename' '
	cd repo &&
	git show --format="" HEAD >actual &&
	grep "a\[1\].txt" actual
'

test_done
