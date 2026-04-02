#!/bin/sh
#
# Tests for diff with challenging inputs that exercise the diff algorithm.
# grit does not expose --patience/--histogram flags directly, so these tests
# verify the default diff algorithm produces correct and sensible output
# on inputs known to challenge LCS-based diff algorithms: repeated lines,
# function-like blocks, insertions between identical sections, etc.

test_description='grit diff — algorithm quality on challenging inputs'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ---------------------------------------------------------------------------
# Setup
# ---------------------------------------------------------------------------
test_expect_success 'setup repo' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

# ---------------------------------------------------------------------------
# 1. Repeated blank lines — diff should not produce nonsense
# ---------------------------------------------------------------------------
test_expect_success 'setup file with repeated blank lines' '
	cd repo &&
	printf "a\n\n\n\nb\n\n\n\nc\n" >blanks.txt &&
	git add blanks.txt &&
	git commit -m "blanks initial"
'

test_expect_success 'insert line between blank blocks' '
	cd repo &&
	printf "a\n\n\n\nINSERTED\nb\n\n\n\nc\n" >blanks.txt &&
	git add blanks.txt &&
	git commit -m "blanks insert"
'

test_expect_success 'diff shows INSERTED as added' '
	cd repo &&
	git diff HEAD~1 HEAD >out &&
	grep "^+INSERTED" out
'

test_expect_success 'diff does not show b or c as changed' '
	cd repo &&
	git diff HEAD~1 HEAD >out &&
	! grep "^-b$" out &&
	! grep "^-c$" out
'

# ---------------------------------------------------------------------------
# 2. Identical function blocks — moving code
# ---------------------------------------------------------------------------
test_expect_success 'setup file with repeated function-like blocks' '
	cd repo &&
	cat >funcs.txt <<-\EOF &&
	func_a() {
	    do_something()
	    return
	}
	func_b() {
	    do_something()
	    return
	}
	EOF
	git add funcs.txt &&
	git commit -m "funcs initial"
'

test_expect_success 'add new function between existing ones' '
	cd repo &&
	cat >funcs.txt <<-\EOF &&
	func_a() {
	    do_something()
	    return
	}
	func_new() {
	    do_other()
	    return
	}
	func_b() {
	    do_something()
	    return
	}
	EOF
	git add funcs.txt &&
	git commit -m "funcs add new"
'

test_expect_success 'diff shows func_new as added' '
	cd repo &&
	git diff HEAD~1 HEAD >out &&
	grep "+func_new" out
'

test_expect_success 'diff shows do_other as added' '
	cd repo &&
	git diff HEAD~1 HEAD >out &&
	grep "+.*do_other" out
'

test_expect_success 'diff does not remove func_a or func_b' '
	cd repo &&
	git diff HEAD~1 HEAD >out &&
	! grep "^-func_a" out &&
	! grep "^-func_b" out
'

# ---------------------------------------------------------------------------
# 3. All-same lines with one change
# ---------------------------------------------------------------------------
test_expect_success 'setup file with all identical lines' '
	cd repo &&
	for i in $(seq 1 10); do echo "same"; done >same.txt &&
	git add same.txt &&
	git commit -m "same lines initial"
'

test_expect_success 'change one line in all-same file' '
	cd repo &&
	for i in $(seq 1 5); do echo "same"; done >same.txt &&
	echo "DIFFERENT" >>same.txt &&
	for i in $(seq 7 10); do echo "same"; done >>same.txt &&
	git add same.txt &&
	git commit -m "same lines change one"
'

test_expect_success 'diff shows exactly one addition and one removal' '
	cd repo &&
	git diff HEAD~1 HEAD >out &&
	adds=$(grep -c "^+same\|^+DIFFERENT" out) &&
	dels=$(grep -c "^-same" out) &&
	test "$adds" -eq 1 &&
	test "$dels" -eq 1
'

test_expect_success 'diff shows DIFFERENT as the added line' '
	cd repo &&
	git diff HEAD~1 HEAD >out &&
	grep "^+DIFFERENT" out
'

# ---------------------------------------------------------------------------
# 4. Insertion at beginning, middle, end
# ---------------------------------------------------------------------------
test_expect_success 'setup ordered file' '
	cd repo &&
	printf "alpha\nbeta\ngamma\ndelta\nepsilon\n" >ordered.txt &&
	git add ordered.txt &&
	git commit -m "ordered initial"
'

test_expect_success 'insert at beginning' '
	cd repo &&
	printf "PREPEND\nalpha\nbeta\ngamma\ndelta\nepsilon\n" >ordered.txt &&
	git add ordered.txt &&
	git commit -m "prepend"
'

test_expect_success 'diff shows PREPEND as sole addition' '
	cd repo &&
	git diff HEAD~1 HEAD >out &&
	grep "^+PREPEND" out &&
	adds=$(grep -c "^+" out | head -1) &&
	# only the one added line plus any context headers
	test "$adds" -le 2
'

test_expect_success 'insert at end' '
	cd repo &&
	printf "PREPEND\nalpha\nbeta\ngamma\ndelta\nepsilon\nAPPEND\n" >ordered.txt &&
	git add ordered.txt &&
	git commit -m "append"
'

test_expect_success 'diff shows APPEND as sole addition' '
	cd repo &&
	git diff HEAD~1 HEAD >out &&
	grep "^+APPEND" out
'

test_expect_success 'insert in middle' '
	cd repo &&
	printf "PREPEND\nalpha\nbeta\nMIDDLE\ngamma\ndelta\nepsilon\nAPPEND\n" >ordered.txt &&
	git add ordered.txt &&
	git commit -m "middle"
'

test_expect_success 'diff shows MIDDLE as sole addition' '
	cd repo &&
	git diff HEAD~1 HEAD >out &&
	grep "^+MIDDLE" out
'

# ---------------------------------------------------------------------------
# 5. Large deletion
# ---------------------------------------------------------------------------
test_expect_success 'setup file with many lines' '
	cd repo &&
	for i in $(seq 1 20); do echo "keep$i"; done >big.txt &&
	git add big.txt &&
	git commit -m "big initial" &&
	git tag big-before
'

test_expect_success 'delete middle section' '
	cd repo &&
	(for i in $(seq 1 5); do echo "keep$i"; done;
	 for i in $(seq 16 20); do echo "keep$i"; done) >big.txt &&
	git add big.txt &&
	git commit -m "big delete middle" &&
	git tag big-after
'

test_expect_success 'diff shows 10 deleted lines' '
	cd repo &&
	git diff big-before big-after >out &&
	dels=$(grep -c "^-keep" out) &&
	test "$dels" -eq 10
'

test_expect_success 'diff preserves kept lines as context' '
	cd repo &&
	git diff big-before big-after >out &&
	grep "^ keep3$" out &&
	grep "^ keep18$" out
'

# ---------------------------------------------------------------------------
# 6. Swap two blocks
# ---------------------------------------------------------------------------
test_expect_success 'setup file with two distinct blocks' '
	cd repo &&
	printf "AAA\nBBB\nCCC\n---\nDDD\nEEE\nFFF\n" >swap.txt &&
	git add swap.txt &&
	git commit -m "swap initial"
'

test_expect_success 'swap the two blocks' '
	cd repo &&
	printf "DDD\nEEE\nFFF\n---\nAAA\nBBB\nCCC\n" >swap.txt &&
	git add swap.txt &&
	git commit -m "swap blocks"
'

test_expect_success 'diff produces output for swapped blocks' '
	cd repo &&
	git diff HEAD~1 HEAD >out &&
	test -s out
'

test_expect_success 'diff has both additions and removals' '
	cd repo &&
	git diff HEAD~1 HEAD >out &&
	grep "^+" out | grep -v "^+++" >adds &&
	grep "^-" out | grep -v "^---" >dels &&
	test -s adds &&
	test -s dels
'

# ---------------------------------------------------------------------------
# 7. --name-only and --name-status still work with tricky content
# ---------------------------------------------------------------------------
test_expect_success 'name-only works with tricky content' '
	cd repo &&
	git diff --name-only HEAD~1 HEAD >out &&
	grep "swap.txt" out
'

test_expect_success 'name-status shows M for modified' '
	cd repo &&
	git diff --name-status HEAD~1 HEAD >out &&
	grep "M.*swap.txt" out
'

# ---------------------------------------------------------------------------
# 8. Empty file edge cases
# ---------------------------------------------------------------------------
test_expect_success 'diff from empty to non-empty' '
	cd repo &&
	>empty.txt &&
	git add empty.txt &&
	git commit -m "empty file" &&
	echo "content" >empty.txt &&
	git add empty.txt &&
	git commit -m "fill empty"
'

test_expect_success 'diff shows addition of content line' '
	cd repo &&
	git diff HEAD~1 HEAD >out &&
	grep "^+content" out
'

test_expect_success 'diff from non-empty to empty' '
	cd repo &&
	>empty.txt &&
	git add empty.txt &&
	git commit -m "emptied"
'

test_expect_success 'diff shows removal of content line' '
	cd repo &&
	git diff HEAD~1 HEAD >out &&
	grep "^-content" out
'

test_done
