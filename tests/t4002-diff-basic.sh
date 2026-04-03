#!/bin/sh

test_description='grit diff basic output structure — headers, hunks, modes

Tests the fundamental structure of unified diff output: diff headers,
index lines, --- and +++ markers, @@ hunk headers, context lines,
additions and deletions, mode changes, and /dev/null handling.'

. ./test-lib.sh

REAL_GIT=/usr/bin/git

# ============================================================
# Setup
# ============================================================

test_expect_success 'setup repo with initial content' '
	$REAL_GIT init basic &&
	cd basic &&
	$REAL_GIT config user.name "Test" &&
	$REAL_GIT config user.email "test@test.com" &&
	printf "alpha\nbeta\ngamma\ndelta\nepsilon\n" >file.txt &&
	$REAL_GIT add file.txt &&
	$REAL_GIT commit -m "initial" &&
	printf "alpha\nBETA\ngamma\ndelta\nepsilon\n" >file.txt &&
	$REAL_GIT add file.txt &&
	$REAL_GIT commit -m "modify beta"
'

# ============================================================
# Part 1: diff header structure (commit-to-commit)
# ============================================================

test_expect_success 'diff header has diff --git line' '
	cd basic &&
	grit diff HEAD~1 HEAD >actual &&
	grep "^diff --git a/file.txt b/file.txt" actual
'

test_expect_success 'diff shows index line with abbreviated OIDs' '
	cd basic &&
	grit diff HEAD~1 HEAD >actual &&
	grep "^index [0-9a-f]\{7\}\.\.[0-9a-f]\{7\} " actual
'

test_expect_success 'diff shows file mode on index line' '
	cd basic &&
	grit diff HEAD~1 HEAD >actual &&
	grep "^index.*100644$" actual
'

test_expect_success 'diff shows --- a/ header' '
	cd basic &&
	grit diff HEAD~1 HEAD >actual &&
	grep -- "^--- a/file.txt$" actual
'

test_expect_success 'diff shows +++ b/ header' '
	cd basic &&
	grit diff HEAD~1 HEAD >actual &&
	grep -- "^+++ b/file.txt$" actual
'

# ============================================================
# Part 2: hunk headers and content
# ============================================================

test_expect_success 'diff shows @@ hunk header' '
	cd basic &&
	grit diff HEAD~1 HEAD >actual &&
	grep "^@@.*@@" actual
'

test_expect_success 'hunk header has correct format @@ -N,M +N,M @@' '
	cd basic &&
	grit diff HEAD~1 HEAD >actual &&
	grep "^@@ -[0-9].*+[0-9].*@@" actual
'

test_expect_success 'single line change shows deletion and addition' '
	cd basic &&
	grit diff HEAD~1 HEAD >actual &&
	grep "^-beta$" actual &&
	grep "^+BETA$" actual
'

test_expect_success 'context lines are shown with space prefix' '
	cd basic &&
	grit diff HEAD~1 HEAD >actual &&
	grep "^ alpha$" actual &&
	grep "^ gamma$" actual
'

# ============================================================
# Part 3: multiple hunks
# ============================================================

test_expect_success 'setup file with many lines for multi-hunk' '
	cd basic &&
	seq 1 30 >numbers.txt &&
	$REAL_GIT add numbers.txt &&
	$REAL_GIT commit -m "add numbers" &&
	cp numbers.txt numbers-mod.txt &&
	sed -i "s/^3$/THREE/" numbers-mod.txt &&
	sed -i "s/^28$/TWENTY-EIGHT/" numbers-mod.txt &&
	mv numbers-mod.txt numbers.txt &&
	$REAL_GIT add numbers.txt &&
	$REAL_GIT commit -m "modify two distant lines"
'

test_expect_success 'multiple changes produce multiple hunks' '
	cd basic &&
	grit diff HEAD~1 HEAD >actual &&
	count=$(grep -c "^@@" actual) &&
	test "$count" -ge 2
'

test_expect_success 'each hunk has its own header with line numbers' '
	cd basic &&
	grit diff HEAD~1 HEAD >actual &&
	grep "^@@ -" actual >hunks &&
	test_line_count = 2 hunks
'

test_expect_success 'first hunk shows line 3 change' '
	cd basic &&
	grit diff HEAD~1 HEAD >actual &&
	grep "^-3$" actual &&
	grep "^+THREE$" actual
'

test_expect_success 'second hunk shows line 28 change' '
	cd basic &&
	grit diff HEAD~1 HEAD >actual &&
	grep "^-28$" actual &&
	grep "^+TWENTY-EIGHT$" actual
'

# ============================================================
# Part 4: new file (--cached)
# ============================================================

test_expect_success 'new file shows /dev/null in --- line' '
	cd basic &&
	echo "brand new" >new.txt &&
	$REAL_GIT add new.txt &&
	grit diff --cached >actual &&
	grep -- "--- /dev/null" actual
'

test_expect_success 'new file shows new file mode header' '
	cd basic &&
	grit diff --cached >actual &&
	grep "^new file mode" actual
'

test_expect_success 'new file diff shows only additions' '
	cd basic &&
	grit diff --cached >actual &&
	grep "^+brand new$" actual
'

test_expect_success 'commit new file for later tests' '
	cd basic &&
	$REAL_GIT commit -m "add new.txt"
'

# ============================================================
# Part 5: deleted file
# ============================================================

test_expect_success 'delete file and stage it' '
	cd basic &&
	$REAL_GIT rm new.txt
'

test_expect_success 'deleted file shows /dev/null in +++ line' '
	cd basic &&
	grit diff --cached >actual &&
	grep -- "+++ /dev/null" actual
'

test_expect_success 'deleted file shows deleted file mode header' '
	cd basic &&
	grit diff --cached >actual &&
	grep "^deleted file mode" actual
'

test_expect_success 'deleted file diff shows only deletions' '
	cd basic &&
	grit diff --cached >actual &&
	grep "^-brand new$" actual
'

test_expect_success 'commit deletion' '
	cd basic &&
	$REAL_GIT commit -m "remove new.txt"
'

# ============================================================
# Part 6: empty diff
# ============================================================

test_expect_success 'clean working tree produces empty diff' '
	cd basic &&
	grit diff >actual &&
	test_line_count = 0 actual
'

test_expect_success 'clean index produces empty cached diff' '
	cd basic &&
	grit diff --cached >actual &&
	test_line_count = 0 actual
'

# ============================================================
# Part 7: context lines (-U) — commit-to-commit
# ============================================================

test_expect_success 'setup single-line change for context tests' '
	cd basic &&
	cp numbers.txt numbers-ctx.txt &&
	sed -i "s/^15$/FIFTEEN/" numbers-ctx.txt &&
	mv numbers-ctx.txt numbers.txt &&
	$REAL_GIT add numbers.txt &&
	$REAL_GIT commit -m "change line 15"
'

test_expect_success '-U0 shows no context lines around change' '
	cd basic &&
	grit diff -U0 HEAD~1 HEAD >actual &&
	grep "^-15$" actual &&
	grep "^+FIFTEEN$" actual &&
	! grep "^ 14$" actual &&
	! grep "^ 16$" actual
'

test_expect_success '-U1 shows 1 context line each side' '
	cd basic &&
	grit diff -U1 HEAD~1 HEAD >actual &&
	grep "^ 14$" actual &&
	grep "^ 16$" actual &&
	! grep "^ 13$" actual
'

test_expect_success 'default context is 3 lines' '
	cd basic &&
	grit diff HEAD~1 HEAD >actual &&
	grep "^ 12$" actual &&
	grep "^ 18$" actual &&
	! grep "^ 11$" actual &&
	! grep "^ 19$" actual
'

# ============================================================
# Part 8: diff between arbitrary commits
# ============================================================

test_expect_success 'diff between two non-adjacent commits produces valid patch' '
	cd basic &&
	grit diff HEAD~3 HEAD >actual &&
	grep "^diff --git" actual
'

test_expect_success 'diff shows multiple files when both changed' '
	cd basic &&
	grit diff --name-only HEAD~3 HEAD >actual &&
	grep "numbers.txt" actual
'

test_expect_success 'diff HEAD~1 HEAD shows the line 15 change' '
	cd basic &&
	grit diff HEAD~1 HEAD >actual &&
	grep "^-15$" actual &&
	grep "^+FIFTEEN$" actual
'

test_done
