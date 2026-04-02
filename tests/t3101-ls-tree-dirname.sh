#!/bin/sh
# Tests for ls-tree with directory names and the -d flag.

test_description='ls-tree directory name handling and -d flag'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ── Setup ──────────────────────────────────────────────────────────────

test_expect_success 'setup repo with nested dirs' '
	grit init repo &&
	cd repo &&
	git config user.email "t@t.com" &&
	git config user.name "T" &&
	mkdir -p a/b/c &&
	mkdir -p d/e &&
	mkdir -p f &&
	echo "file-root" >root.txt &&
	echo "file-a" >a/file-a.txt &&
	echo "file-b" >a/b/file-b.txt &&
	echo "file-c" >a/b/c/file-c.txt &&
	echo "file-d" >d/file-d.txt &&
	echo "file-e" >d/e/file-e.txt &&
	echo "file-f" >f/file-f.txt &&
	grit add . &&
	grit commit -m "initial commit"
'

# ── -d flag: show only tree entries ────────────────────────────────────

test_expect_success 'ls-tree -d shows only top-level trees' '
	cd repo &&
	grit ls-tree -d HEAD >actual &&
	grep "^040000 tree" actual &&
	! grep "^100644 blob" actual
'

test_expect_success 'ls-tree -d lists correct directory names' '
	cd repo &&
	grit ls-tree -d HEAD >actual &&
	grep "	a$" actual &&
	grep "	d$" actual &&
	grep "	f$" actual
'

test_expect_success 'ls-tree -d does not show root blobs' '
	cd repo &&
	grit ls-tree -d HEAD >actual &&
	! grep "root.txt" actual
'

test_expect_success 'ls-tree -d count matches number of top-level dirs' '
	cd repo &&
	grit ls-tree -d HEAD >actual &&
	test_line_count = 3 actual
'

# ── -d combined with -r ────────────────────────────────────────────────

test_expect_success 'ls-tree -d -r shows no entries (only trees filtered but -r recurses into blobs)' '
	cd repo &&
	grit ls-tree -d -r HEAD >actual &&
	test_line_count = 0 actual
'

# ── Without -d: default shows trees + blobs at top level ──────────────

test_expect_success 'ls-tree HEAD shows both blobs and trees at top level' '
	cd repo &&
	grit ls-tree HEAD >actual &&
	grep "blob" actual &&
	grep "tree" actual
'

test_expect_success 'ls-tree HEAD top level has 4 entries (3 dirs + 1 file)' '
	cd repo &&
	grit ls-tree HEAD >actual &&
	test_line_count = 4 actual
'

# ── Directory as path argument ─────────────────────────────────────────

test_expect_success 'ls-tree HEAD a shows tree entry for a' '
	cd repo &&
	grit ls-tree HEAD a >actual &&
	grep "	a$" actual &&
	test_line_count = 1 actual
'

test_expect_success 'ls-tree HEAD with nonexistent dir produces no output' '
	cd repo &&
	grit ls-tree HEAD nonexistent >actual &&
	test_line_count = 0 actual
'

# ── -d with path filter ───────────────────────────────────────────────

test_expect_success 'ls-tree -d HEAD a shows the tree for a' '
	cd repo &&
	grit ls-tree -d HEAD a >actual &&
	grep "	a$" actual &&
	test_line_count = 1 actual
'

test_expect_success 'ls-tree -d HEAD root.txt shows nothing (blob filtered)' '
	cd repo &&
	grit ls-tree -d HEAD root.txt >actual &&
	test_line_count = 0 actual
'

# ── -d with --name-only ───────────────────────────────────────────────

test_expect_success 'ls-tree -d --name-only shows only directory names' '
	cd repo &&
	grit ls-tree -d --name-only HEAD >actual &&
	echo "a" >expect &&
	echo "d" >>expect &&
	echo "f" >>expect &&
	test_cmp expect actual
'

# ── Multiple dirs at same depth ───────────────────────────────────────

test_expect_success 'setup repo with several peer dirs' '
	grit init multi-repo &&
	cd multi-repo &&
	git config user.email "t@t.com" &&
	git config user.name "T" &&
	mkdir alpha beta gamma &&
	echo x >alpha/x.txt &&
	echo y >beta/y.txt &&
	echo z >gamma/z.txt &&
	grit add . &&
	grit commit -m "peer dirs"
'

test_expect_success 'ls-tree -d shows all peer dirs' '
	cd multi-repo &&
	grit ls-tree -d HEAD >actual &&
	test_line_count = 3 actual &&
	grep "	alpha$" actual &&
	grep "	beta$" actual &&
	grep "	gamma$" actual
'

test_expect_success 'ls-tree -d HEAD alpha lists just alpha' '
	cd multi-repo &&
	grit ls-tree -d HEAD alpha >actual &&
	test_line_count = 1 actual
'

# ── Empty tree behaviour ─────────────────────────────────────────────

test_expect_success 'setup repo with only a file (no dirs)' '
	grit init no-dirs &&
	cd no-dirs &&
	git config user.email "t@t.com" &&
	git config user.name "T" &&
	echo only >only.txt &&
	grit add only.txt &&
	grit commit -m "no dirs"
'

test_expect_success 'ls-tree -d on tree with no subdirs gives empty output' '
	cd no-dirs &&
	grit ls-tree -d HEAD >actual &&
	test_line_count = 0 actual
'

test_expect_success 'ls-tree (no -d) still shows the blob' '
	cd no-dirs &&
	grit ls-tree HEAD >actual &&
	test_line_count = 1 actual &&
	grep "only.txt" actual
'

# ── Deeply nested: -d doesn't recurse ────────────────────────────────

test_expect_success 'ls-tree -d on deep nesting shows only top-level tree' '
	cd repo &&
	grit ls-tree -d HEAD >actual &&
	! grep "a/b" actual &&
	! grep "a/b/c" actual &&
	! grep "d/e" actual
'

test_done
