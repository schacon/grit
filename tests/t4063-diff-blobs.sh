#!/bin/sh
# Test diff between blob objects via diff-tree and diff with commit pairs.
# Exercises tree-level diffing, blob content comparison, and raw output.

test_description='diff between blob objects and trees'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ── Setup ──────────────────────────────────────────────────────────────────────

test_expect_success 'setup repo with blob changes across commits' '
	grit init repo &&
	cd repo &&
	git config user.email "t@t.com" &&
	git config user.name "T" &&
	echo "line1" >a.txt &&
	echo "line1" >b.txt &&
	grit add a.txt b.txt &&
	grit commit -m "c1" &&
	grit rev-parse HEAD >../c1 &&
	echo "line2" >>a.txt &&
	grit add a.txt &&
	grit commit -m "c2" &&
	grit rev-parse HEAD >../c2 &&
	echo "line2" >>b.txt &&
	echo "line3" >>a.txt &&
	grit add a.txt b.txt &&
	grit commit -m "c3" &&
	grit rev-parse HEAD >../c3 &&
	echo "new" >c.txt &&
	grit add c.txt &&
	grit commit -m "c4-add" &&
	grit rev-parse HEAD >../c4 &&
	grit rm b.txt &&
	grit commit -m "c5-del" &&
	grit rev-parse HEAD >../c5
'

# ── diff-tree raw output ─────────────────────────────────────────────────────

test_expect_success 'diff-tree raw shows blob OID changes' '
	cd repo &&
	c1=$(cat ../c1) && c2=$(cat ../c2) &&
	grit diff-tree "$c1" "$c2" >out &&
	grep "M" out | grep "a.txt" &&
	# should show old and new blob OIDs
	grep "100644 100644" out
'

test_expect_success 'diff-tree raw does not show unchanged files' '
	cd repo &&
	c1=$(cat ../c1) && c2=$(cat ../c2) &&
	grit diff-tree "$c1" "$c2" >out &&
	! grep "b.txt" out
'

test_expect_success 'diff-tree shows added file with A status' '
	cd repo &&
	c3=$(cat ../c3) && c4=$(cat ../c4) &&
	grit diff-tree "$c3" "$c4" >out &&
	grep "A" out | grep "c.txt"
'

test_expect_success 'diff-tree shows deleted file with D status' '
	cd repo &&
	c4=$(cat ../c4) && c5=$(cat ../c5) &&
	grit diff-tree "$c4" "$c5" >out &&
	grep "D" out | grep "b.txt"
'

test_expect_success 'diff-tree raw output has correct format' '
	cd repo &&
	c1=$(cat ../c1) && c2=$(cat ../c2) &&
	grit diff-tree "$c1" "$c2" >out &&
	# format: :old_mode new_mode old_oid new_oid status<TAB>path
	grep "^:" out | head -1 >first &&
	test -s first
'

# ── diff-tree with -p (patch) ────────────────────────────────────────────────

test_expect_success 'diff-tree -p produces unified diff' '
	cd repo &&
	c1=$(cat ../c1) && c2=$(cat ../c2) &&
	grit diff-tree -p "$c1" "$c2" >out &&
	grep "^diff --git" out &&
	grep "^---" out &&
	grep "^+++" out &&
	grep "^@@" out
'

test_expect_success 'diff-tree -p shows content of added lines' '
	cd repo &&
	c1=$(cat ../c1) && c2=$(cat ../c2) &&
	grit diff-tree -p "$c1" "$c2" >out &&
	grep "+line2" out
'

test_expect_success 'diff-tree -p for new file shows all lines as additions' '
	cd repo &&
	c3=$(cat ../c3) && c4=$(cat ../c4) &&
	grit diff-tree -p "$c3" "$c4" >out &&
	grep "+new" out &&
	grep "new file" out
'

test_expect_success 'diff-tree -p for deleted file shows removals' '
	cd repo &&
	c4=$(cat ../c4) && c5=$(cat ../c5) &&
	grit diff-tree -p "$c4" "$c5" >out &&
	grep "deleted file" out &&
	grep "^-line" out
'

# ── diff-tree --name-only / --name-status ─────────────────────────────────────

test_expect_success 'diff-tree --name-only lists filenames' '
	cd repo &&
	c1=$(cat ../c1) && c3=$(cat ../c3) &&
	grit diff-tree --name-only "$c1" "$c3" >out &&
	grep "a.txt" out &&
	grep "b.txt" out
'

test_expect_success 'diff-tree --name-status shows status letters' '
	cd repo &&
	c1=$(cat ../c1) && c3=$(cat ../c3) &&
	grit diff-tree --name-status "$c1" "$c3" >out &&
	grep "M" out
'

# ── diff between commits (high-level) ────────────────────────────────────────

test_expect_success 'diff between two commits shows blob changes' '
	cd repo &&
	c1=$(cat ../c1) && c2=$(cat ../c2) &&
	grit diff "$c1" "$c2" >out &&
	grep "diff --git" out &&
	grep "a.txt" out
'

test_expect_success 'diff with pathspec limits blob comparison' '
	cd repo &&
	c1=$(cat ../c1) && c3=$(cat ../c3) &&
	grit diff "$c1" "$c3" -- a.txt >out &&
	grep "a.txt" out &&
	! grep "b.txt" out
'

test_expect_success 'diff identical commits produces no output' '
	cd repo &&
	c1=$(cat ../c1) &&
	grit diff "$c1" "$c1" >out &&
	! test -s out
'

# ── Blob identity via ls-tree ────────────────────────────────────────────────

test_expect_success 'ls-tree shows blob OIDs for each commit' '
	cd repo &&
	c1=$(cat ../c1) && c2=$(cat ../c2) &&
	grit ls-tree "$c1" >tree1 &&
	grit ls-tree "$c2" >tree2 &&
	grep "blob" tree1 &&
	grep "blob" tree2
'

test_expect_success 'modified file has different blob OID in different commits' '
	cd repo &&
	c1=$(cat ../c1) && c2=$(cat ../c2) &&
	grit ls-tree "$c1" >tree1 &&
	grit ls-tree "$c2" >tree2 &&
	oid1=$(grep "a.txt" tree1 | awk "{print \$3}") &&
	oid2=$(grep "a.txt" tree2 | awk "{print \$3}") &&
	test "$oid1" != "$oid2"
'

test_expect_success 'unchanged file has same blob OID across commits' '
	cd repo &&
	c1=$(cat ../c1) && c2=$(cat ../c2) &&
	grit ls-tree "$c1" >tree1 &&
	grit ls-tree "$c2" >tree2 &&
	oid1=$(grep "b.txt" tree1 | awk "{print \$3}") &&
	oid2=$(grep "b.txt" tree2 | awk "{print \$3}") &&
	test "$oid1" = "$oid2"
'

test_expect_success 'blob content matches via cat-file' '
	cd repo &&
	c2=$(cat ../c2) &&
	oid=$(grit ls-tree "$c2" | grep "a.txt" | awk "{print \$3}") &&
	grit cat-file -p "$oid" >content &&
	grep "line1" content &&
	grep "line2" content
'

test_expect_success 'cat-file -t confirms object is blob' '
	cd repo &&
	c1=$(cat ../c1) &&
	oid=$(grit ls-tree "$c1" | grep "a.txt" | awk "{print \$3}") &&
	grit cat-file -t "$oid" >type &&
	grep "blob" type
'

test_expect_success 'cat-file -s shows blob size' '
	cd repo &&
	c1=$(cat ../c1) &&
	oid=$(grit ls-tree "$c1" | grep "a.txt" | awk "{print \$3}") &&
	grit cat-file -s "$oid" >sz &&
	size=$(cat sz) &&
	test "$size" -gt 0
'

# ── Multiple file changes ────────────────────────────────────────────────────

test_expect_success 'diff-tree shows multiple changes in one diff' '
	cd repo &&
	c1=$(cat ../c1) && c3=$(cat ../c3) &&
	grit diff-tree "$c1" "$c3" >out &&
	grep "a.txt" out &&
	grep "b.txt" out
'

test_expect_success 'diff-tree -p across multiple commits shows all patches' '
	cd repo &&
	c1=$(cat ../c1) && c3=$(cat ../c3) &&
	grit diff-tree -p "$c1" "$c3" >out &&
	count=$(grep "^diff --git" out | wc -l) &&
	test "$count" -eq 2
'

test_done
