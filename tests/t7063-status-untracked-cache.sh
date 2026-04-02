#!/bin/sh
#
# Tests for 'grit status' with untracked files — directory collapsing,
# -u flag modes, porcelain/short output, and various working tree states.
# Ported subset from git/t/t7063-status-untracked-cache.sh (upstream ~58 tests).

test_description='grit status — untracked files handling'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ---------------------------------------------------------------------------
# Setup
# ---------------------------------------------------------------------------
test_expect_success 'setup repository' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo "tracked" >tracked.txt &&
	git add tracked.txt &&
	git commit -m "initial"
'

# ---------------------------------------------------------------------------
# Basic untracked file detection
# ---------------------------------------------------------------------------
test_expect_success 'status detects untracked file' '
	cd repo &&
	echo "new" >untracked.txt &&
	git status --porcelain >../st.out &&
	grep "^?? untracked.txt" ../st.out
'

test_expect_success 'status shows multiple untracked files' '
	cd repo &&
	echo "a" >a-new.txt &&
	echo "b" >b-new.txt &&
	git status --porcelain >../st2.out &&
	grep "^?? a-new.txt" ../st2.out &&
	grep "^?? b-new.txt" ../st2.out
'

test_expect_success 'cleanup untracked files' '
	cd repo &&
	rm -f untracked.txt a-new.txt b-new.txt
'

# ---------------------------------------------------------------------------
# Directory collapsing (normal mode)
# ---------------------------------------------------------------------------
test_expect_success 'untracked directory shown as single entry' '
	cd repo &&
	mkdir -p subdir &&
	echo a >subdir/a.txt &&
	echo b >subdir/b.txt &&
	git status --porcelain >../st3.out &&
	grep "^?? subdir/" ../st3.out
'

test_expect_success 'multiple untracked dirs each shown' '
	cd repo &&
	mkdir -p dir1 dir2 &&
	echo x >dir1/x &&
	echo y >dir2/y &&
	git status --porcelain >../st4.out &&
	grep "^?? dir1/" ../st4.out &&
	grep "^?? dir2/" ../st4.out
'

test_expect_success 'cleanup directories' '
	cd repo &&
	rm -rf subdir dir1 dir2
'

# ---------------------------------------------------------------------------
# -u no: hide untracked
# ---------------------------------------------------------------------------
test_expect_success 'status -u no hides untracked files' '
	cd repo &&
	echo "hidden" >hidden.txt &&
	git status --porcelain -u no >../st5.out &&
	! grep "hidden.txt" ../st5.out &&
	! grep "^??" ../st5.out
'

test_expect_success 'cleanup hidden' '
	cd repo &&
	rm -f hidden.txt
'

# ---------------------------------------------------------------------------
# Deeply nested directories
# ---------------------------------------------------------------------------
test_expect_success 'deeply nested untracked dirs collapsed at top level' '
	cd repo &&
	mkdir -p deep/nested/inner &&
	echo content >deep/nested/inner/file.txt &&
	git status --porcelain >../st6.out &&
	grep "^?? deep/" ../st6.out
'

test_expect_success 'cleanup deep dirs' '
	cd repo &&
	rm -rf deep
'

# ---------------------------------------------------------------------------
# Mix of tracked modifications and untracked
# ---------------------------------------------------------------------------
test_expect_success 'modified tracked file shown with M prefix' '
	cd repo &&
	echo "modified" >tracked.txt &&
	git status --porcelain >../st7.out &&
	grep "^ M tracked.txt" ../st7.out
'

test_expect_success 'modified tracked and new untracked both shown' '
	cd repo &&
	echo "brand-new" >brand-new.txt &&
	git status --porcelain >../st8.out &&
	grep "^ M tracked.txt" ../st8.out &&
	grep "^?? brand-new.txt" ../st8.out
'

# ---------------------------------------------------------------------------
# Staged changes
# ---------------------------------------------------------------------------
test_expect_success 'staged file shown with index marker' '
	cd repo &&
	git add tracked.txt &&
	git status --porcelain >../st9.out &&
	grep "^M  tracked.txt" ../st9.out
'

test_expect_success 'staged and untracked both visible' '
	cd repo &&
	git status --porcelain >../st10.out &&
	grep "^M  tracked.txt" ../st10.out &&
	grep "^?? brand-new.txt" ../st10.out
'

# ---------------------------------------------------------------------------
# Status after commit
# ---------------------------------------------------------------------------
test_expect_success 'commit clears staged status' '
	cd repo &&
	git add brand-new.txt &&
	git commit -m "add files" &&
	git status --porcelain >../st11.out &&
	! grep "^M " ../st11.out &&
	! grep "^A " ../st11.out
'

# ---------------------------------------------------------------------------
# Deleted file
# ---------------------------------------------------------------------------
test_expect_success 'status shows deleted tracked file' '
	cd repo &&
	rm tracked.txt &&
	git status --porcelain >../st12.out &&
	grep "^ D tracked.txt" ../st12.out
'

test_expect_success 'status shows staged deletion' '
	cd repo &&
	git rm tracked.txt >/dev/null 2>&1 &&
	git status --porcelain >../st13.out &&
	grep "^D  tracked.txt" ../st13.out
'

test_expect_success 'commit deletion' '
	cd repo &&
	git commit -m "remove tracked"
'

# ---------------------------------------------------------------------------
# Porcelain format structure
# ---------------------------------------------------------------------------
test_expect_success 'porcelain lines have two-char status prefix' '
	cd repo &&
	echo "test" >format-test.txt &&
	git status --porcelain >../st14.out &&
	grep "^?? format-test.txt" ../st14.out
'

# ---------------------------------------------------------------------------
# Status -sb shows branch header
# ---------------------------------------------------------------------------
test_expect_success 'status -sb shows branch header' '
	cd repo &&
	git status -sb >../st15.out &&
	head -n 1 ../st15.out | grep "^## master"
'

# ---------------------------------------------------------------------------
# Status -z (NUL termination)
# ---------------------------------------------------------------------------
test_expect_success 'status -z output contains entries' '
	cd repo &&
	git status --porcelain -z >../st16.out &&
	tr "\0" "\n" <../st16.out >../st16-decoded.out &&
	grep "format-test.txt" ../st16-decoded.out
'

# ---------------------------------------------------------------------------
# Status after adding file
# ---------------------------------------------------------------------------
test_expect_success 'added file shows as staged new' '
	cd repo &&
	git add format-test.txt &&
	git status --porcelain >../st17.out &&
	grep "^A  format-test.txt" ../st17.out
'

# ---------------------------------------------------------------------------
# Renamed file
# ---------------------------------------------------------------------------
test_expect_success 'mv shows rename in status' '
	cd repo &&
	git commit -m "add format-test" &&
	git mv format-test.txt renamed-test.txt &&
	git status --porcelain >../st18.out &&
	grep "renamed-test.txt" ../st18.out
'

# ---------------------------------------------------------------------------
# Status with both index and worktree changes
# ---------------------------------------------------------------------------
test_expect_success 'file modified in both index and worktree' '
	cd repo &&
	git commit -m "rename" &&
	echo "staged-content" >renamed-test.txt &&
	git add renamed-test.txt &&
	echo "worktree-content" >renamed-test.txt &&
	git status --porcelain >../st19.out &&
	grep "^MM renamed-test.txt" ../st19.out
'

# ---------------------------------------------------------------------------
# Restore via checkout
# ---------------------------------------------------------------------------
test_expect_success 'restore undoes worktree modification' '
	cd repo &&
	git checkout -- renamed-test.txt &&
	git status --porcelain >../st20.out &&
	grep "^M  renamed-test.txt" ../st20.out &&
	! grep "^MM" ../st20.out
'

# ---------------------------------------------------------------------------
# Restore --staged
# ---------------------------------------------------------------------------
test_expect_success 'restore --staged unstages file' '
	cd repo &&
	git restore --staged renamed-test.txt &&
	git status --porcelain >../st21.out &&
	grep "^ M renamed-test.txt" ../st21.out
'

# ---------------------------------------------------------------------------
# Clean state
# ---------------------------------------------------------------------------
test_expect_success 'checkout all gives clean status' '
	cd repo &&
	git checkout -- . &&
	git status --porcelain >../st22.out &&
	! grep "^[MADRCU]" ../st22.out
'

test_done
