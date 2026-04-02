#!/bin/sh
#
# t0050-filesystem.sh — filesystem edge cases: case sensitivity, unicode,
# special characters, long filenames, symlinks, empty directories
#

test_description='filesystem edge cases'
. ./test-lib.sh

# ── setup ────────────────────────────────────────────────────────────────────

test_expect_success 'setup: init repo' '
	git init fs-repo &&
	cd fs-repo &&
	git config user.name "Test" &&
	git config user.email "test@test.com"
'

# ── case sensitivity (Linux is case-sensitive) ───────────────────────────────

test_expect_success 'case-different filenames are tracked separately' '
	cd fs-repo &&
	echo "lower" >readme.txt &&
	echo "upper" >README.txt &&
	git add readme.txt README.txt &&
	git status >status-out &&
	grep "readme.txt" status-out &&
	grep "README.txt" status-out
'

test_expect_success 'case-different files commit independently' '
	cd fs-repo &&
	git commit -m "case files" &&
	git ls-files >ls-out &&
	grep "^readme.txt$" ls-out &&
	grep "^README.txt$" ls-out
'

test_expect_success 'case-different files have different content' '
	cd fs-repo &&
	git show HEAD:readme.txt >actual-lower &&
	git show HEAD:README.txt >actual-upper &&
	echo "lower" >expect-lower &&
	echo "upper" >expect-upper &&
	test_cmp expect-lower actual-lower &&
	test_cmp expect-upper actual-upper
'

# ── unicode filenames ────────────────────────────────────────────────────────

test_expect_success 'unicode filename can be added and committed' '
	cd fs-repo &&
	printf "café content\n" >"café.txt" &&
	git add "café.txt" &&
	git commit -m "unicode filename" &&
	git ls-files >ls-out &&
	grep "café.txt" ls-out
'

test_expect_success 'unicode filename content is retrievable' '
	cd fs-repo &&
	git show "HEAD:café.txt" >actual &&
	printf "café content\n" >expect &&
	test_cmp expect actual
'

test_expect_success 'multiple unicode filenames' '
	cd fs-repo &&
	echo "umlaut" >"über.txt" &&
	echo "cjk" >"日本語.txt" &&
	echo "emoji" >"🎉.txt" &&
	git add "über.txt" "日本語.txt" "🎉.txt" &&
	git commit -m "various unicode names" &&
	git ls-files >ls-out &&
	grep "über.txt" ls-out &&
	grep "日本語.txt" ls-out &&
	grep "🎉.txt" ls-out
'

# ── filenames with spaces and special characters ─────────────────────────────

test_expect_success 'filename with spaces' '
	cd fs-repo &&
	echo "spaced" >"file with spaces.txt" &&
	git add "file with spaces.txt" &&
	git commit -m "spaces in name" &&
	git ls-files >ls-out &&
	grep "file with spaces.txt" ls-out
'

test_expect_success 'filename with dashes and underscores' '
	cd fs-repo &&
	echo "dashed" >"file-with-dashes.txt" &&
	echo "under" >"file_underscores.txt" &&
	git add "file-with-dashes.txt" "file_underscores.txt" &&
	git commit -m "dashes and underscores" &&
	git ls-files >ls-out &&
	grep "file-with-dashes.txt" ls-out &&
	grep "file_underscores.txt" ls-out
'

test_expect_success 'filename starting with dot' '
	cd fs-repo &&
	echo "hidden" >".hidden-file" &&
	git add ".hidden-file" &&
	git commit -m "dotfile" &&
	git ls-files >ls-out &&
	grep "^\.hidden-file$" ls-out
'

test_expect_success 'filename with multiple dots' '
	cd fs-repo &&
	echo "multi" >"file.test.backup.txt" &&
	git add "file.test.backup.txt" &&
	git commit -m "multi dot" &&
	git show HEAD:"file.test.backup.txt" >actual &&
	echo "multi" >expect &&
	test_cmp expect actual
'

# ── long filenames ───────────────────────────────────────────────────────────

test_expect_success 'long filename (200 chars) can be tracked' '
	cd fs-repo &&
	longname=$(python3 -c "print(\"a\" * 200)") &&
	echo "long" >"$longname" &&
	git add "$longname" &&
	git commit -m "long filename" &&
	git ls-files >ls-out &&
	grep "aaaaaaaaa" ls-out
'

# ── deeply nested paths ─────────────────────────────────────────────────────

test_expect_success 'deeply nested directory path' '
	cd fs-repo &&
	mkdir -p a/b/c/d/e/f/g &&
	echo "deep" >a/b/c/d/e/f/g/file.txt &&
	git add a/b/c/d/e/f/g/file.txt &&
	git commit -m "deep nesting" &&
	git show HEAD:a/b/c/d/e/f/g/file.txt >actual &&
	echo "deep" >expect &&
	test_cmp expect actual
'

# ── symlinks ─────────────────────────────────────────────────────────────────

test_expect_success 'symlink can be added' '
	cd fs-repo &&
	echo "target content" >target-file &&
	ln -s target-file link-file &&
	git add target-file link-file &&
	git commit -m "add symlink" &&
	git ls-files >ls-out &&
	grep "link-file" ls-out &&
	grep "target-file" ls-out
'

test_expect_success 'symlink target is stored correctly' '
	cd fs-repo &&
	git cat-file -p HEAD >commit-tree &&
	tree_hash=$(head -1 commit-tree | awk "{print \$2}") &&
	git ls-tree HEAD -- link-file >tree-entry &&
	grep "120000" tree-entry
'

# ── empty directories ────────────────────────────────────────────────────────

test_expect_success 'empty directory is not tracked in index' '
	cd fs-repo &&
	mkdir -p empty-dir &&
	git add empty-dir 2>/dev/null;
	git ls-files >ls-out &&
	! grep "empty-dir" ls-out
'

# ── binary files ─────────────────────────────────────────────────────────────

test_expect_success 'binary file (null bytes) can be committed' '
	cd fs-repo &&
	printf "\x00\x01\x02\xff" >binary.dat &&
	git add binary.dat &&
	git commit -m "binary file" &&
	git ls-files >ls-out &&
	grep "binary.dat" ls-out
'

test_expect_success 'binary file round-trips through checkout' '
	cd fs-repo &&
	git show HEAD:binary.dat >retrieved.dat &&
	cmp binary.dat retrieved.dat
'

# ── files in root vs subdirectory with same name ─────────────────────────────

test_expect_success 'same filename in root and subdir are independent' '
	cd fs-repo &&
	echo "root version" >shared-name.txt &&
	mkdir -p sub &&
	echo "sub version" >sub/shared-name.txt &&
	git add shared-name.txt sub/shared-name.txt &&
	git commit -m "same name different paths" &&
	git show HEAD:shared-name.txt >actual-root &&
	git show HEAD:sub/shared-name.txt >actual-sub &&
	echo "root version" >expect-root &&
	echo "sub version" >expect-sub &&
	test_cmp expect-root actual-root &&
	test_cmp expect-sub actual-sub
'

# ── executable bit ───────────────────────────────────────────────────────────

test_expect_success 'executable permission is tracked' '
	cd fs-repo &&
	echo "#!/bin/sh" >script.sh &&
	chmod +x script.sh &&
	git add script.sh &&
	git commit -m "executable script" &&
	git ls-tree HEAD -- script.sh >tree-out &&
	grep "100755" tree-out
'

test_expect_success 'non-executable file has 100644 mode' '
	cd fs-repo &&
	git ls-tree HEAD -- shared-name.txt >tree-out &&
	grep "100644" tree-out
'

test_done
