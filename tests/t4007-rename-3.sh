#!/bin/sh

test_description='grit diff with renames across 3+ commits

Tests diff behavior when files are renamed across multiple commits,
including chains of renames, renames with modifications at each step,
and diffing non-adjacent commits where a file has been renamed multiple
times. Since grit lacks -M rename detection, renames appear as D+A pairs.'

. ./test-lib.sh

REAL_GIT=/usr/bin/git

# ============================================================
# Setup: chain of renames across 4 commits
# ============================================================

test_expect_success 'setup repo with initial files' '
	$REAL_GIT init rename3 &&
	cd rename3 &&
	$REAL_GIT config user.name "Test" &&
	$REAL_GIT config user.email "test@test.com" &&
	printf "line1\nline2\nline3\nline4\nline5\n" >alpha.txt &&
	echo "constant" >stable.txt &&
	$REAL_GIT add alpha.txt stable.txt &&
	$REAL_GIT commit -m "C1: initial"
'

test_expect_success 'C2: rename alpha -> beta' '
	cd rename3 &&
	$REAL_GIT mv alpha.txt beta.txt &&
	$REAL_GIT commit -m "C2: rename alpha to beta"
'

test_expect_success 'C3: rename beta -> gamma with modification' '
	cd rename3 &&
	$REAL_GIT mv beta.txt gamma.txt &&
	printf "line1\nLINE2\nline3\nline4\nline5\n" >gamma.txt &&
	$REAL_GIT add gamma.txt &&
	$REAL_GIT commit -m "C3: rename beta to gamma + modify"
'

test_expect_success 'C4: rename gamma -> delta' '
	cd rename3 &&
	$REAL_GIT mv gamma.txt delta.txt &&
	$REAL_GIT commit -m "C4: rename gamma to delta"
'

# ============================================================
# Adjacent commit diffs
# ============================================================

test_expect_success 'C1->C2: alpha deleted, beta added' '
	cd rename3 &&
	grit diff --name-status HEAD~3 HEAD~2 >actual &&
	grep "^D.*alpha.txt" actual &&
	grep "^A.*beta.txt" actual
'

test_expect_success 'C2->C3: beta deleted, gamma added' '
	cd rename3 &&
	grit diff --name-status HEAD~2 HEAD~1 >actual &&
	grep "^D.*beta.txt" actual &&
	grep "^A.*gamma.txt" actual
'

test_expect_success 'C3->C4: gamma deleted, delta added' '
	cd rename3 &&
	grit diff --name-status HEAD~1 HEAD >actual &&
	grep "^D.*gamma.txt" actual &&
	grep "^A.*delta.txt" actual
'

# ============================================================
# Non-adjacent commit diffs (spanning multiple renames)
# ============================================================

test_expect_success 'C1->C3: alpha deleted, gamma added' '
	cd rename3 &&
	grit diff --name-status HEAD~3 HEAD~1 >actual &&
	grep "^D.*alpha.txt" actual &&
	grep "^A.*gamma.txt" actual
'

test_expect_success 'C1->C4: alpha deleted, delta added' '
	cd rename3 &&
	grit diff --name-status HEAD~3 HEAD >actual &&
	grep "^D.*alpha.txt" actual &&
	grep "^A.*delta.txt" actual
'

test_expect_success 'C1->C4: no intermediate names appear' '
	cd rename3 &&
	grit diff --name-only HEAD~3 HEAD >actual &&
	! grep "beta.txt" actual &&
	! grep "gamma.txt" actual
'

test_expect_success 'C2->C4: beta deleted, delta added' '
	cd rename3 &&
	grit diff --name-status HEAD~2 HEAD >actual &&
	grep "^D.*beta.txt" actual &&
	grep "^A.*delta.txt" actual
'

# ============================================================
# Stable file should not appear in diffs
# ============================================================

test_expect_success 'stable.txt does not appear in C1->C4 diff' '
	cd rename3 &&
	grit diff --name-only HEAD~3 HEAD >actual &&
	! grep "stable.txt" actual
'

# ============================================================
# Full patch output across renames
# ============================================================

test_expect_success 'C1->C4 patch shows deleted file mode for alpha' '
	cd rename3 &&
	grit diff HEAD~3 HEAD >actual &&
	grep "^deleted file mode" actual
'

test_expect_success 'C1->C4 patch shows new file mode for delta' '
	cd rename3 &&
	grit diff HEAD~3 HEAD >actual &&
	grep "^new file mode" actual
'

test_expect_success 'C1->C4 patch includes the modification made in C3' '
	cd rename3 &&
	grit diff HEAD~3 HEAD >actual &&
	grep "+LINE2" actual
'

test_expect_success 'C1->C4 --stat shows both alpha and delta' '
	cd rename3 &&
	grit diff --stat HEAD~3 HEAD >actual &&
	grep "alpha.txt" actual &&
	grep "delta.txt" actual
'

test_expect_success 'C1->C4 --numstat shows deletions for alpha' '
	cd rename3 &&
	grit diff --numstat HEAD~3 HEAD >actual &&
	grep "alpha.txt" actual
'

# ============================================================
# Rename + add new file with old name
# ============================================================

test_expect_success 'C5: add new file with a previously-used name' '
	cd rename3 &&
	echo "I am new alpha" >alpha.txt &&
	$REAL_GIT add alpha.txt &&
	$REAL_GIT commit -m "C5: new alpha.txt"
'

test_expect_success 'C1->C5: both old and new alpha.txt in diff' '
	cd rename3 &&
	grit diff --name-status HEAD~4 HEAD >actual &&
	grep "alpha.txt" actual
'

test_expect_success 'C4->C5: new alpha.txt is added' '
	cd rename3 &&
	grit diff --name-status HEAD~1 HEAD >actual &&
	grep "^A.*alpha.txt" actual
'

# ============================================================
# Diff-tree plumbing across renames
# ============================================================

test_expect_success 'diff-tree -r C1 C4 shows D and A' '
	cd rename3 &&
	grit diff-tree -r HEAD~4 HEAD~1 >actual &&
	grep "D	alpha.txt" actual &&
	grep "A	delta.txt" actual
'

test_expect_success 'diff-tree -r C1 C4 preserves OIDs' '
	cd rename3 &&
	grit diff-tree -r HEAD~4 HEAD~1 >actual &&
	grep "[0-9a-f]\{40\}" actual
'

# ============================================================
# Rename to/from subdirectories across commits
# ============================================================

test_expect_success 'setup subdir rename chain' '
	cd rename3 &&
	mkdir -p sub1 sub2 &&
	echo "traveler" >sub1/file.txt &&
	$REAL_GIT add sub1/file.txt &&
	$REAL_GIT commit -m "C6: file in sub1" &&
	$REAL_GIT mv sub1/file.txt sub2/file.txt &&
	$REAL_GIT commit -m "C7: move to sub2" &&
	$REAL_GIT mv sub2/file.txt file-top.txt &&
	$REAL_GIT commit -m "C8: move to top level"
'

test_expect_success 'C6->C8: sub1/file.txt deleted, file-top.txt added' '
	cd rename3 &&
	grit diff --name-status HEAD~2 HEAD >actual &&
	grep "^D.*sub1/file.txt" actual &&
	grep "^A.*file-top.txt" actual
'

test_expect_success 'C6->C8: no intermediate sub2/file.txt' '
	cd rename3 &&
	grit diff --name-only HEAD~2 HEAD >actual &&
	! grep "sub2/file.txt" actual
'

test_expect_success 'C6->C8 patch has correct paths' '
	cd rename3 &&
	grit diff HEAD~2 HEAD >actual &&
	grep -- "--- a/sub1/file.txt" actual &&
	grep -- "+++ b/file-top.txt" actual
'

test_done
