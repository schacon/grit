#!/bin/sh
# Test diff with file type changes: file→symlink, symlink→file,
# mode changes, and related transitions.

test_description='diff with type changes (file↔symlink, mode changes)'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repo' '
	grit init repo &&
	cd repo &&
	git config user.email "test@test.com" &&
	git config user.name "Test"
'

# ── file to symlink ──

test_expect_success 'create regular file and commit' '
	cd repo &&
	echo "original content" >target.txt &&
	grit add target.txt &&
	grit commit -m "add target.txt"
'

test_expect_success 'replace file with symlink and diff --cached' '
	cd repo &&
	rm target.txt &&
	ln -s /dev/null target.txt &&
	grit add target.txt &&
	grit diff --cached >actual &&
	grep "old mode 100644" actual &&
	grep "new mode 120000" actual
'

test_expect_success 'diff --name-status shows typechange as cached' '
	cd repo &&
	grit diff --name-status --cached >actual &&
	cat actual &&
	grep "target.txt" actual
'

test_expect_success 'commit typechange file→symlink' '
	cd repo &&
	grit commit -m "file to symlink"
'

test_expect_success 'diff between commits shows mode change' '
	cd repo &&
	grit diff HEAD~1 HEAD >actual &&
	grep "old mode 100644" actual &&
	grep "new mode 120000" actual
'

test_expect_success 'diff --stat between commits with typechange' '
	cd repo &&
	grit diff --stat HEAD~1 HEAD >actual &&
	grep "target.txt" actual
'

test_expect_success 'diff --name-only between commits with typechange' '
	cd repo &&
	grit diff --name-only HEAD~1 HEAD >actual &&
	grep "target.txt" actual
'

# ── symlink to file ──

test_expect_success 'replace symlink with regular file' '
	cd repo &&
	rm target.txt &&
	echo "back to regular" >target.txt &&
	grit add target.txt &&
	grit diff --cached >actual &&
	grep "old mode 120000" actual &&
	grep "new mode 100644" actual
'

test_expect_success 'commit symlink→file' '
	cd repo &&
	grit commit -m "symlink back to file"
'

test_expect_success 'diff two commits apart shows file restored' '
	cd repo &&
	grit diff HEAD~2 HEAD >actual &&
	grep "target.txt" actual
'

# ── symlink to different target ──

test_expect_success 'create and commit symlink' '
	cd repo &&
	ln -s target.txt link-a.txt &&
	grit add link-a.txt &&
	grit commit -m "add symlink link-a"
'

test_expect_success 'change symlink target and diff' '
	cd repo &&
	rm link-a.txt &&
	ln -s /dev/null link-a.txt &&
	grit add link-a.txt &&
	grit diff --cached >actual &&
	grep "link-a.txt" actual
'

test_expect_success 'commit changed symlink' '
	cd repo &&
	grit commit -m "change symlink target"
'

# ── multiple typechanges in one diff ──

test_expect_success 'setup multiple files for typechange' '
	cd repo &&
	echo "file-a" >a.txt &&
	echo "file-b" >b.txt &&
	ln -s a.txt c.txt &&
	grit add a.txt b.txt c.txt &&
	grit commit -m "add a, b, c"
'

test_expect_success 'typechange multiple files at once' '
	cd repo &&
	rm a.txt && ln -s /dev/null a.txt &&
	rm c.txt && echo "now a file" >c.txt &&
	grit add a.txt c.txt &&
	grit diff --cached --name-only >actual &&
	grep "a.txt" actual &&
	grep "c.txt" actual
'

test_expect_success 'diff --cached shows both typechanges' '
	cd repo &&
	grit diff --cached >actual &&
	grep -c "diff --git" actual >count &&
	test "$(cat count)" -ge 2
'

test_expect_success 'commit multiple typechanges' '
	cd repo &&
	grit commit -m "multiple typechanges"
'

# ── new symlink ──

test_expect_success 'add new symlink and diff cached' '
	cd repo &&
	ln -s b.txt new-link.txt &&
	grit add new-link.txt &&
	grit diff --cached >actual &&
	grep "new file mode 120000" actual
'

test_expect_success 'commit new symlink' '
	cd repo &&
	grit commit -m "add new-link"
'

# ── delete symlink ──

test_expect_success 'delete symlink and diff cached' '
	cd repo &&
	rm new-link.txt &&
	grit add new-link.txt &&
	grit diff --cached >actual &&
	grep "deleted file mode 120000" actual
'

test_expect_success 'commit deleted symlink' '
	cd repo &&
	grit commit -m "delete new-link"
'

# ── working tree diff with typechange ──

test_expect_success 'unstaged typechange shows in diff' '
	cd repo &&
	echo "regular" >regular.txt &&
	grit add regular.txt &&
	grit commit -m "add regular" &&
	rm regular.txt &&
	ln -s b.txt regular.txt &&
	grit diff >actual &&
	grep "regular.txt" actual
'

test_expect_success 'diff --name-status shows unstaged typechange' '
	cd repo &&
	grit diff --name-status >actual &&
	grep "regular.txt" actual
'

test_expect_success 'cleanup unstaged typechange' '
	cd repo &&
	rm regular.txt &&
	echo "regular" >regular.txt
'

# ── symlink pointing to existing target then deleted ──

test_expect_success 'create symlink to existing file' '
	cd repo &&
	ln -s b.txt valid-link.txt &&
	grit add valid-link.txt &&
	grit diff --cached >actual &&
	grep "valid-link.txt" actual &&
	grep "120000" actual
'

test_expect_success 'commit valid symlink' '
	cd repo &&
	grit commit -m "valid symlink"
'

# ── diff --stat with typechange ──

test_expect_success 'diff --stat shows typechange info' '
	cd repo &&
	rm valid-link.txt &&
	echo "now a real file" >valid-link.txt &&
	grit add valid-link.txt &&
	grit diff --stat --cached >actual &&
	grep "valid-link.txt" actual
'

test_expect_success 'diff --numstat shows typechange info' '
	cd repo &&
	grit diff --numstat --cached >actual &&
	grep "valid-link.txt" actual
'

test_expect_success 'commit valid-link typechange' '
	cd repo &&
	grit commit -m "valid-link to file"
'

# ── long symlink target ──

test_expect_success 'symlink with long target path' '
	cd repo &&
	mkdir -p a/very/deep/directory/structure &&
	echo "deep" >a/very/deep/directory/structure/file.txt &&
	grit add a/ &&
	grit commit -m "deep dir" &&
	ln -s a/very/deep/directory/structure/file.txt long-link.txt &&
	grit add long-link.txt &&
	grit diff --cached >actual &&
	grep "long-link.txt" actual &&
	grep "120000" actual
'

test_expect_success 'commit long symlink' '
	cd repo &&
	grit commit -m "long symlink"
'

test_done
