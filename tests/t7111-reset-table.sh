#!/bin/sh
# Systematic test of reset behavior table: --soft, --mixed, --hard
# with combinations of HEAD state, index state, and working tree state.
# Verifies HEAD movement, index contents, and working tree contents
# for each mode.

test_description='reset behavior table (soft/mixed/hard with different states)'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ── Setup ──────────────────────────────────────────────────────────────────────

test_expect_success 'create baseline repo with 3 commits' '
	grit init repo &&
	cd repo &&
	git config user.email "t@t.com" &&
	git config user.name "T" &&
	echo "v1" >file.txt &&
	grit add file.txt &&
	grit commit -m "c1" &&
	grit rev-parse HEAD >../c1 &&
	echo "v2" >file.txt &&
	grit add file.txt &&
	grit commit -m "c2" &&
	grit rev-parse HEAD >../c2 &&
	echo "v3" >file.txt &&
	grit add file.txt &&
	grit commit -m "c3" &&
	grit rev-parse HEAD >../c3
'

# ══════════════════════════════════════════════════════════════════════════════
# SOFT RESET: moves HEAD only; index and working tree unchanged
# ══════════════════════════════════════════════════════════════════════════════

test_expect_success 'soft: HEAD moves to target' '
	cd repo &&
	grit reset --soft $(cat ../c1) &&
	test "$(grit rev-parse HEAD)" = "$(cat ../c1)"
'

test_expect_success 'soft: working tree keeps HEAD~0 content' '
	cd repo &&
	cat file.txt >actual &&
	echo "v3" >expect &&
	test_cmp expect actual
'

test_expect_success 'soft: index still has c3 content (staged diff)' '
	cd repo &&
	grit diff --cached >out &&
	grep "+v3" out
'

test_expect_success 'soft: can recommit to restore' '
	cd repo &&
	grit commit -m "restore-c3" &&
	cat file.txt >actual &&
	echo "v3" >expect &&
	test_cmp expect actual
'

# After soft recommit, HEAD has v3 content. Record it for mixed tests.
test_expect_success 'record c3b for mixed tests' '
	cd repo &&
	grit rev-parse HEAD >../c3b
'

# ══════════════════════════════════════════════════════════════════════════════
# MIXED RESET (default): moves HEAD and resets index; working tree unchanged
# ══════════════════════════════════════════════════════════════════════════════

test_expect_success 'mixed: HEAD moves to target' '
	cd repo &&
	grit reset --mixed $(cat ../c1) &&
	test "$(grit rev-parse HEAD)" = "$(cat ../c1)"
'

test_expect_success 'mixed: working tree keeps latest content' '
	cd repo &&
	cat file.txt >actual &&
	echo "v3" >expect &&
	test_cmp expect actual
'

test_expect_success 'mixed: index matches target (no staged diff)' '
	cd repo &&
	grit diff --cached >out &&
	! test -s out
'

test_expect_success 'mixed: working tree diff shows changes' '
	cd repo &&
	grit diff >out &&
	grep "+v3" out
'

# Restore for next section
test_expect_success 'restore to c3 for hard tests' '
	cd repo &&
	grit add file.txt &&
	grit commit -m "back-to-c3-again" &&
	grit rev-parse HEAD >../c3c
'

# ══════════════════════════════════════════════════════════════════════════════
# HARD RESET: moves HEAD, resets index AND working tree
# ══════════════════════════════════════════════════════════════════════════════

test_expect_success 'hard: HEAD moves to target' '
	cd repo &&
	grit reset --hard $(cat ../c1) &&
	test "$(grit rev-parse HEAD)" = "$(cat ../c1)"
'

test_expect_success 'hard: working tree matches target content' '
	cd repo &&
	cat file.txt >actual &&
	echo "v1" >expect &&
	test_cmp expect actual
'

test_expect_success 'hard: index matches target (no staged diff)' '
	cd repo &&
	grit diff --cached >out &&
	! test -s out
'

test_expect_success 'hard: no working tree diff either' '
	cd repo &&
	grit diff >out &&
	! test -s out
'

# ══════════════════════════════════════════════════════════════════════════════
# RESET with dirty working tree
# ══════════════════════════════════════════════════════════════════════════════

test_expect_success 'setup: create commits and dirty working tree' '
	cd repo &&
	echo "v2" >file.txt &&
	grit add file.txt &&
	grit commit -m "c2-redo" &&
	grit rev-parse HEAD >../c2r &&
	echo "dirty" >file.txt
'

test_expect_success 'soft with dirty worktree: HEAD moves, worktree untouched' '
	cd repo &&
	grit reset --soft $(cat ../c1) &&
	test "$(grit rev-parse HEAD)" = "$(cat ../c1)" &&
	cat file.txt >actual &&
	echo "dirty" >expect &&
	test_cmp expect actual
'

test_expect_success 'restore for mixed dirty test' '
	cd repo &&
	echo "v2" >file.txt &&
	grit add file.txt &&
	grit commit -m "c2-redo2" &&
	echo "dirty2" >file.txt
'

test_expect_success 'mixed with dirty worktree: index reset, worktree untouched' '
	cd repo &&
	grit reset --mixed $(cat ../c1) &&
	test "$(grit rev-parse HEAD)" = "$(cat ../c1)" &&
	cat file.txt >actual &&
	echo "dirty2" >expect &&
	test_cmp expect actual
'

test_expect_success 'restore for hard dirty test' '
	cd repo &&
	grit add file.txt &&
	grit commit -m "c2-redo3" &&
	echo "dirty3" >file.txt
'

test_expect_success 'hard with dirty worktree: everything overwritten' '
	cd repo &&
	grit reset --hard $(cat ../c1) &&
	test "$(grit rev-parse HEAD)" = "$(cat ../c1)" &&
	cat file.txt >actual &&
	echo "v1" >expect &&
	test_cmp expect actual
'

# ══════════════════════════════════════════════════════════════════════════════
# RESET with new (untracked) files
# ══════════════════════════════════════════════════════════════════════════════

test_expect_success 'setup: commit and add untracked file' '
	cd repo &&
	echo "v2" >file.txt &&
	grit add file.txt &&
	grit commit -m "c2-for-untracked" &&
	echo "untracked" >new.txt
'

test_expect_success 'soft reset does not remove untracked files' '
	cd repo &&
	grit reset --soft $(cat ../c1) &&
	test -f new.txt
'

test_expect_success 'mixed reset does not remove untracked files' '
	cd repo &&
	grit add file.txt &&
	grit commit -m "tmp" &&
	grit reset --mixed $(cat ../c1) &&
	test -f new.txt
'

test_expect_success 'hard reset does not remove untracked files' '
	cd repo &&
	grit add file.txt &&
	grit commit -m "tmp2" &&
	grit reset --hard $(cat ../c1) &&
	test -f new.txt
'

# ══════════════════════════════════════════════════════════════════════════════
# RESET to HEAD (no-op style)
# ══════════════════════════════════════════════════════════════════════════════

test_expect_success 'soft reset to HEAD is a no-op' '
	cd repo &&
	before=$(grit rev-parse HEAD) &&
	grit reset --soft HEAD &&
	after=$(grit rev-parse HEAD) &&
	test "$before" = "$after"
'

test_expect_success 'mixed reset to HEAD unstages changes' '
	cd repo &&
	echo "staged" >file.txt &&
	grit add file.txt &&
	grit reset --mixed HEAD &&
	grit diff --cached >out &&
	! test -s out
'

test_expect_success 'hard reset to HEAD discards working tree changes' '
	cd repo &&
	echo "modified" >file.txt &&
	grit reset --hard HEAD &&
	cat file.txt >actual &&
	echo "v1" >expect &&
	test_cmp expect actual
'

# ══════════════════════════════════════════════════════════════════════════════
# DEFAULT mode (no flag = --mixed)
# ══════════════════════════════════════════════════════════════════════════════

test_expect_success 'setup for default mode test' '
	cd repo &&
	echo "v2" >file.txt &&
	grit add file.txt &&
	grit commit -m "for-default" &&
	grit rev-parse HEAD >../cdef
'

test_expect_success 'reset without flag behaves as --mixed' '
	cd repo &&
	grit reset $(cat ../c1) &&
	test "$(grit rev-parse HEAD)" = "$(cat ../c1)" &&
	grit diff --cached >out &&
	! test -s out &&
	cat file.txt >actual &&
	echo "v2" >expect &&
	test_cmp expect actual
'

test_done
