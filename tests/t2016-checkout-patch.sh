#!/bin/sh
# Test checkout -p (interactive patch mode).

test_description='grit checkout -p (patch mode)'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

###########################################################################
# Section 1: Setup
###########################################################################

test_expect_success 'setup: create repo with content' '
	grit init patch-repo &&
	cd patch-repo &&
	grit config user.email "test@test.com" &&
	grit config user.name "Test" &&
	echo "line1" >file.txt &&
	echo "line2" >>file.txt &&
	echo "line3" >>file.txt &&
	grit add file.txt &&
	grit commit -m "initial" &&
	echo "another" >other.txt &&
	grit add other.txt &&
	grit commit -m "add other"
'

###########################################################################
# Section 2: checkout -p with no changes
###########################################################################

test_expect_success 'checkout -p with clean tree says no changes' '
	cd patch-repo &&
	echo "y" | grit checkout -p 2>err &&
	# Should succeed (no changes to discard)
	true
'

test_expect_success 'checkout -p with no changes exits 0' '
	cd patch-repo &&
	echo "n" | grit checkout -p
'

###########################################################################
# Section 3: checkout -p discarding changes
###########################################################################

test_expect_success 'checkout -p: accept (y) reverts single hunk' '
	cd patch-repo &&
	echo "modified-line1" >file.txt &&
	echo "line2" >>file.txt &&
	echo "line3" >>file.txt &&
	echo "y" | grit checkout -p -- file.txt &&
	echo "line1" >expected &&
	echo "line2" >>expected &&
	echo "line3" >>expected &&
	test_cmp expected file.txt
'

test_expect_success 'checkout -p: reject (n) keeps changes' '
	cd patch-repo &&
	echo "modified" >file.txt &&
	echo "n" | grit checkout -p -- file.txt &&
	echo "modified" >expected &&
	test_cmp expected file.txt &&
	grit checkout -- file.txt
'

test_expect_success 'checkout -p: quit (q) keeps remaining changes' '
	cd patch-repo &&
	echo "changed" >file.txt &&
	echo "q" | grit checkout -p -- file.txt &&
	echo "changed" >expected &&
	test_cmp expected file.txt &&
	grit checkout -- file.txt
'

###########################################################################
# Section 4: checkout -p with multiple files
###########################################################################

test_expect_success 'checkout -p with multiple modified files' '
	cd patch-repo &&
	echo "mod1" >file.txt &&
	echo "mod2" >other.txt &&
	printf "y\ny\n" | grit checkout -p &&
	echo "line1" >exp1 &&
	echo "line2" >>exp1 &&
	echo "line3" >>exp1 &&
	echo "another" >exp2 &&
	test_cmp exp1 file.txt &&
	test_cmp exp2 other.txt
'

test_expect_success 'checkout -p: accept first, reject second' '
	cd patch-repo &&
	echo "mod1" >file.txt &&
	echo "mod2" >other.txt &&
	printf "y\nn\n" | grit checkout -p &&
	echo "line1" >exp1 &&
	echo "line2" >>exp1 &&
	echo "line3" >>exp1 &&
	test_cmp exp1 file.txt &&
	echo "mod2" >exp2 &&
	test_cmp exp2 other.txt &&
	grit checkout -- other.txt
'

test_expect_success 'checkout -p: reject first, accept second' '
	cd patch-repo &&
	echo "mod1" >file.txt &&
	echo "mod2" >other.txt &&
	printf "n\ny\n" | grit checkout -p &&
	echo "mod1" >exp1 &&
	test_cmp exp1 file.txt &&
	echo "another" >exp2 &&
	test_cmp exp2 other.txt &&
	grit checkout -- file.txt
'

###########################################################################
# Section 5: checkout -p with accept-all (a) and discard-all (d)
###########################################################################

test_expect_success 'checkout -p: accept-all (a) discards all hunks in current file' '
	cd patch-repo &&
	echo "changed1" >file.txt &&
	printf "a\ny\n" | grit checkout -p &&
	echo "line1" >exp1 &&
	echo "line2" >>exp1 &&
	echo "line3" >>exp1 &&
	echo "another" >exp2 &&
	test_cmp exp1 file.txt &&
	test_cmp exp2 other.txt
'

test_expect_success 'checkout -p: discard-all (d) keeps remaining hunks for this file' '
	cd patch-repo &&
	echo "changed1" >file.txt &&
	echo "d" | grit checkout -p -- file.txt &&
	echo "changed1" >expected &&
	test_cmp expected file.txt &&
	grit checkout -- file.txt
'

###########################################################################
# Section 6: checkout -p with new/deleted files
###########################################################################

test_expect_success 'checkout -p ignores untracked files' '
	cd patch-repo &&
	echo "untracked" >newfile.txt &&
	echo "n" | grit checkout -p &&
	test -f newfile.txt &&
	rm newfile.txt
'

test_expect_success 'checkout -p with deleted tracked file' '
	cd patch-repo &&
	rm file.txt &&
	echo "y" | grit checkout -p -- file.txt &&
	test -f file.txt &&
	echo "line1" >expected &&
	echo "line2" >>expected &&
	echo "line3" >>expected &&
	test_cmp expected file.txt
'

###########################################################################
# Section 7: checkout -p with specific path
###########################################################################

test_expect_success 'checkout -p -- specific-file only shows that file' '
	cd patch-repo &&
	echo "mod1" >file.txt &&
	echo "mod2" >other.txt &&
	echo "y" | grit checkout -p -- file.txt &&
	echo "line1" >exp1 &&
	echo "line2" >>exp1 &&
	echo "line3" >>exp1 &&
	test_cmp exp1 file.txt &&
	echo "mod2" >exp2 &&
	test_cmp exp2 other.txt &&
	grit checkout -- other.txt
'

###########################################################################
# Section 8: checkout -p from a specific commit
###########################################################################

test_expect_success 'setup: create more history' '
	cd patch-repo &&
	echo "v2-line1" >file.txt &&
	echo "v2-line2" >>file.txt &&
	grit add file.txt &&
	grit commit -m "v2 of file"
'

test_expect_success 'checkout -p HEAD~1 -- file restores from that commit' '
	cd patch-repo &&
	echo "y" | grit checkout -p HEAD~1 -- file.txt &&
	echo "line1" >expected &&
	echo "line2" >>expected &&
	echo "line3" >>expected &&
	test_cmp expected file.txt &&
	grit checkout -- file.txt
'

###########################################################################
# Section 9: checkout -p in subdirectory
###########################################################################

test_expect_success 'setup: create subdirectory with files' '
	cd patch-repo &&
	mkdir -p sub &&
	echo "subfile" >sub/s.txt &&
	grit add sub/s.txt &&
	grit commit -m "add subdir"
'

test_expect_success 'checkout -p works on subdirectory files' '
	cd patch-repo &&
	echo "modified-sub" >sub/s.txt &&
	echo "y" | grit checkout -p -- sub/s.txt &&
	echo "subfile" >expected &&
	test_cmp expected sub/s.txt
'

###########################################################################
# Section 10: Edge cases
###########################################################################

test_expect_success 'checkout -p with empty input keeps changes' '
	cd patch-repo &&
	echo "dirty" >file.txt &&
	echo "" | grit checkout -p -- file.txt &&
	grit checkout -- file.txt
'

test_expect_success 'checkout -p on binary-like content works' '
	cd patch-repo &&
	printf "line1\nline2\nline3\n" >bin.txt &&
	grit add bin.txt &&
	grit commit -m "add bin" &&
	printf "mod1\nline2\nmod3\n" >bin.txt &&
	echo "y" | grit checkout -p -- bin.txt &&
	printf "line1\nline2\nline3\n" >expected &&
	test_cmp expected bin.txt
'

test_expect_success 'checkout -p twice in a row is idempotent when clean' '
	cd patch-repo &&
	echo "y" | grit checkout -p 2>err1 &&
	echo "y" | grit checkout -p 2>err2
'

test_expect_success 'checkout -p preserves other staged changes' '
	cd patch-repo &&
	echo "staged" >staged.txt &&
	grit add staged.txt &&
	echo "unstaged-mod" >file.txt &&
	echo "y" | grit checkout -p -- file.txt &&
	grit status >out &&
	grep "staged.txt" out &&
	grit checkout -- file.txt &&
	grit reset HEAD staged.txt &&
	rm -f staged.txt
'

test_done
