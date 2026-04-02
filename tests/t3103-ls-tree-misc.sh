#!/bin/sh
# Tests for ls-tree miscellaneous options: -r, -t, -l, --name-only, -z, --format, path filtering.

test_description='ls-tree miscellaneous options'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ── Setup ──────────────────────────────────────────────────────────────

test_expect_success 'setup repo with nested structure' '
	grit init repo &&
	cd repo &&
	git config user.email "t@t.com" &&
	git config user.name "T" &&
	mkdir -p dir/sub &&
	echo "root-file" >root.txt &&
	echo "dir-file" >dir/file.txt &&
	echo "sub-file" >dir/sub/deep.txt &&
	grit add . &&
	grit commit -m "initial"
'

# ── -r: recurse into subtrees ─────────────────────────────────────────

test_expect_success 'ls-tree -r shows all blobs recursively' '
	cd repo &&
	grit ls-tree -r HEAD >actual &&
	grep "root.txt" actual &&
	grep "dir/file.txt" actual &&
	grep "dir/sub/deep.txt" actual
'

test_expect_success 'ls-tree -r shows only blobs, no tree entries' '
	cd repo &&
	grit ls-tree -r HEAD >actual &&
	! grep "^040000 tree" actual
'

test_expect_success 'ls-tree -r lists 3 entries for our repo' '
	cd repo &&
	grit ls-tree -r HEAD >actual &&
	test_line_count = 3 actual
'

# ── -r -t: recurse but also show tree entries ─────────────────────────

test_expect_success 'ls-tree -r -t shows trees and blobs' '
	cd repo &&
	grit ls-tree -r -t HEAD >actual &&
	grep "^040000 tree" actual &&
	grep "^100644 blob" actual
'

test_expect_success 'ls-tree -r -t lists 5 entries (2 trees + 3 blobs)' '
	cd repo &&
	grit ls-tree -r -t HEAD >actual &&
	test_line_count = 5 actual
'

test_expect_success 'ls-tree -r -t tree entries appear before their children' '
	cd repo &&
	grit ls-tree -r -t HEAD >actual &&
	dir_line=$(grep -n "	dir$" actual | head -1 | cut -d: -f1) &&
	file_line=$(grep -n "dir/file.txt" actual | head -1 | cut -d: -f1) &&
	test "$dir_line" -lt "$file_line"
'

# ── -l / --long: show object size ─────────────────────────────────────

test_expect_success 'ls-tree -l HEAD shows size column' '
	cd repo &&
	grit ls-tree -l HEAD >actual &&
	# long format has an extra column between OID and name
	awk "{print NF}" actual | while read n; do
		test "$n" -ge 5 || return 1
	done
'

test_expect_success 'ls-tree -l -r shows sizes for all blobs' '
	cd repo &&
	grit ls-tree -l -r HEAD >actual &&
	test_line_count = 3 actual
'

# ── --name-only ────────────────────────────────────────────────────────

test_expect_success 'ls-tree --name-only shows only names at top level' '
	cd repo &&
	grit ls-tree --name-only HEAD >actual &&
	echo "dir" >expect &&
	echo "root.txt" >>expect &&
	test_cmp expect actual
'

test_expect_success 'ls-tree -r --name-only shows full paths' '
	cd repo &&
	grit ls-tree -r --name-only HEAD >actual &&
	echo "dir/file.txt" >expect &&
	echo "dir/sub/deep.txt" >>expect &&
	echo "root.txt" >>expect &&
	test_cmp expect actual
'

test_expect_success 'ls-tree -d --name-only shows only dir names' '
	cd repo &&
	grit ls-tree -d --name-only HEAD >actual &&
	echo "dir" >expect &&
	test_cmp expect actual
'

# ── -z: NUL-terminated output ─────────────────────────────────────────

test_expect_success 'ls-tree -z terminates entries with NUL' '
	cd repo &&
	grit ls-tree -z HEAD >actual &&
	nul_count=$(tr -cd "\0" <actual | wc -c) &&
	test "$nul_count" -eq 2
'

test_expect_success 'ls-tree -z -r terminates all entries with NUL' '
	cd repo &&
	grit ls-tree -z -r HEAD >actual &&
	nul_count=$(tr -cd "\0" <actual | wc -c) &&
	test "$nul_count" -eq 3
'

# ── --format ───────────────────────────────────────────────────────────

test_expect_success 'ls-tree --format with %(objectname) and %(path)' '
	cd repo &&
	grit ls-tree --format="%(objectname) %(path)" HEAD >actual &&
	# Should have OID + space + path on each line
	test_line_count = 2 actual &&
	grep "root.txt" actual &&
	grep "dir" actual
'

test_expect_success 'ls-tree --format with %(objecttype)' '
	cd repo &&
	grit ls-tree --format="%(objecttype) %(path)" HEAD >actual &&
	grep "^blob root.txt$" actual &&
	grep "^tree dir$" actual
'

test_expect_success 'ls-tree --format with %(objectmode)' '
	cd repo &&
	grit ls-tree --format="%(objectmode) %(path)" HEAD >actual &&
	grep "^100644 root.txt$" actual &&
	grep "^040000 dir$" actual
'

# ── Path filtering ─────────────────────────────────────────────────────

test_expect_success 'ls-tree HEAD with specific file path' '
	cd repo &&
	grit ls-tree HEAD root.txt >actual &&
	test_line_count = 1 actual &&
	grep "root.txt" actual
'

test_expect_success 'ls-tree HEAD with dir path shows tree entry' '
	cd repo &&
	grit ls-tree HEAD dir >actual &&
	test_line_count = 1 actual &&
	grep "	dir$" actual
'

test_expect_success 'ls-tree HEAD with nonexistent path gives empty output' '
	cd repo &&
	grit ls-tree HEAD no-such-file >actual &&
	test_line_count = 0 actual
'

# ── Using tree OID directly ───────────────────────────────────────────

test_expect_success 'ls-tree with explicit tree OID works' '
	cd repo &&
	tree_oid=$(grit rev-parse HEAD^{tree}) &&
	grit ls-tree "$tree_oid" >actual &&
	grit ls-tree HEAD >expect &&
	test_cmp expect actual
'

# ── ls-tree on specific commit ────────────────────────────────────────

test_expect_success 'setup second commit' '
	cd repo &&
	grit rev-parse HEAD >old_head &&
	echo "new" >new.txt &&
	grit add new.txt &&
	grit commit -m "add new.txt"
'

test_expect_success 'ls-tree old commit shows old tree' '
	cd repo &&
	old=$(cat old_head) &&
	grit ls-tree "$old" >actual &&
	! grep "new.txt" actual &&
	grep "root.txt" actual
'

test_expect_success 'ls-tree HEAD shows new tree' '
	cd repo &&
	grit ls-tree HEAD >actual &&
	grep "new.txt" actual &&
	grep "root.txt" actual
'

test_done
