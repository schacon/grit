#!/bin/sh
# Ported from git/t/t7512-status-help.sh (partially)
# Tests for 'grit status' output, advice messages, and formatting.

test_description='grit status advice and help messages'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test.com"
'

# ── initial/empty repository status ──────────────────────────────────────

test_expect_success 'status on empty repo shows No commits yet' '
	git init empty_repo &&
	cd empty_repo &&
	git status >../actual_empty &&
	grep "No commits yet" ../actual_empty
'

test_expect_success 'status on empty repo shows On branch' '
	grep "On branch master" actual_empty
'

# ── clean working tree ────────────────────────────────────────────────────

test_expect_success 'status after commit shows nothing to commit' '
	git init clean_repo &&
	cd clean_repo &&
	git config user.name T && git config user.email t@t &&
	echo init >file.txt &&
	git add file.txt &&
	git commit -m "initial" 2>/dev/null &&
	git status >../actual_clean &&
	grep "nothing to commit" ../actual_clean
'

test_expect_success 'status shows On branch master' '
	grep "On branch master" actual_clean
'

# ── unstaged modifications ────────────────────────────────────────────────

test_expect_success 'status shows modified file in Changes not staged' '
	cd repo &&
	echo init >file.txt && git add file.txt && git commit -m init 2>/dev/null &&
	echo modified >>file.txt &&
	git status >actual &&
	grep "Changes not staged for commit" actual &&
	grep "modified:   file.txt" actual
'

test_expect_success 'status advice for unstaged: use git add' '
	cd repo &&
	git status >actual &&
	grep "git add" actual
'

test_expect_success 'status advice for unstaged: use git commit -a' '
	cd repo &&
	git status >actual &&
	grep "git commit -a" actual
'

# ── staged changes ────────────────────────────────────────────────────────

test_expect_success 'status shows staged file in Changes to be committed' '
	cd repo &&
	git add file.txt &&
	echo extra >>file.txt && git add file.txt &&
	git status >actual &&
	grep "Changes to be committed" actual &&
	grep "modified:   file.txt" actual
'

test_expect_success 'status advice for staged: use git restore --staged to unstage' '
	cd repo &&
	git status >actual &&
	grep "git restore --staged" actual
'

# ── untracked files ───────────────────────────────────────────────────────

test_expect_success 'status shows untracked files section' '
	cd repo &&
	echo new >untracked.txt &&
	git status >actual &&
	grep "Untracked files" actual &&
	grep "untracked.txt" actual
'

test_expect_success 'status advice for untracked: use git add' '
	cd repo &&
	git status >actual &&
	grep "git add" actual
'

# ── both staged and unstaged changes ──────────────────────────────────────

test_expect_success 'status shows both staged and unstaged sections' '
	cd repo &&
	git commit -m "commit staged" 2>/dev/null &&
	echo more >>file.txt &&
	git add file.txt &&
	echo even_more >>file.txt &&
	git status >actual &&
	grep "Changes to be committed" actual &&
	grep "Changes not staged for commit" actual
'

# ── short format (-s) ────────────────────────────────────────────────────

test_expect_success 'status -s shows short format' '
	cd repo &&
	git status -s >actual &&
	grep "MM file.txt" actual
'

test_expect_success 'status -s shows ?? for untracked' '
	cd repo &&
	git status -s >actual &&
	grep "?? untracked.txt" actual
'

test_expect_success 'status -s M for staged only' '
	cd repo &&
	git commit -a -m "all" 2>/dev/null &&
	echo staged_only >staged.txt &&
	git add staged.txt &&
	git status -s >actual &&
	grep "^A" actual &&
	grep "staged.txt" actual
'

# ── porcelain format ──────────────────────────────────────────────────────

test_expect_success 'status --porcelain shows branch line with ##' '
	cd repo &&
	git status --porcelain >actual &&
	grep "^## master" actual
'

test_expect_success 'status --porcelain shows status codes' '
	cd repo &&
	echo porcelain_new >pnew.txt &&
	git status --porcelain >actual &&
	grep "?? pnew.txt" actual
'

# ── branch display ────────────────────────────────────────────────────────

test_expect_success 'status -b shows branch in short mode' '
	cd repo &&
	git status -s -b >actual &&
	grep "^## master" actual
'

# ── deleted files ─────────────────────────────────────────────────────────

test_expect_success 'status shows deleted file' '
	cd repo &&
	git add pnew.txt && git commit -m "add pnew" 2>/dev/null &&
	rm pnew.txt &&
	git status >actual &&
	grep "deleted:" actual &&
	grep "pnew.txt" actual
'

test_expect_success 'status -s shows D for deleted' '
	cd repo &&
	git status -s >actual &&
	grep "D" actual &&
	grep "pnew.txt" actual
'

# ── staged deletion ──────────────────────────────────────────────────────

test_expect_success 'status shows staged deletion in Changes to be committed' '
	cd repo &&
	git rm pnew.txt 2>/dev/null &&
	git status >actual &&
	grep "Changes to be committed" actual &&
	grep "deleted:" actual
'

# ── new file staged ──────────────────────────────────────────────────────

test_expect_success 'status shows new file in Changes to be committed' '
	cd repo &&
	git commit -m "del" 2>/dev/null &&
	echo brand_new >brandnew.txt &&
	git add brandnew.txt &&
	git status >actual &&
	grep "Changes to be committed" actual &&
	grep "new file:" actual &&
	grep "brandnew.txt" actual
'

test_expect_success 'status -s shows A for new staged file' '
	cd repo &&
	git status -s >actual &&
	grep "^A" actual &&
	grep "brandnew.txt" actual
'

# ── -u no (hide untracked) ───────────────────────────────────────────────

test_expect_success 'status -u no hides untracked files' '
	cd repo &&
	git status -u no >actual &&
	! grep "Untracked files" actual &&
	! grep "untracked.txt" actual
'

test_expect_success 'status -s -u no hides ?? entries' '
	cd repo &&
	git status -s -u no >actual &&
	! grep "^??" actual
'

# ── multiple modified files ───────────────────────────────────────────────

test_expect_success 'status lists multiple modified files' '
	cd repo &&
	git commit -m "bn" 2>/dev/null &&
	echo a >a.txt && echo b >b.txt && echo c >c.txt &&
	git add a.txt b.txt c.txt && git commit -m "add abc" 2>/dev/null &&
	echo aa >>a.txt && echo bb >>b.txt && echo cc >>c.txt &&
	git status >actual &&
	grep "modified:   a.txt" actual &&
	grep "modified:   b.txt" actual &&
	grep "modified:   c.txt" actual
'

test_expect_success 'status -s lists multiple M entries' '
	cd repo &&
	git status -s >actual &&
	grep "M a.txt" actual &&
	grep "M b.txt" actual &&
	grep "M c.txt" actual
'

# ── status in subdirectory ────────────────────────────────────────────────

test_expect_success 'status from subdirectory shows all changes' '
	cd repo &&
	mkdir -p sub &&
	cd sub &&
	git status >../sub_actual &&
	cd .. &&
	grep "modified:   a.txt" sub_actual
'

# ── status with -z (NUL terminator) ──────────────────────────────────────

test_expect_success 'status -s -z uses NUL terminators' '
	cd repo &&
	git status -s -z >actual &&
	# Should contain NUL bytes: decode to check
	tr "\0" "\n" <actual >decoded &&
	grep "a.txt" decoded
'

# ── status after all clean ───────────────────────────────────────────────

test_expect_success 'status shows clean after committing everything' '
	git init allclean_repo &&
	cd allclean_repo &&
	git config user.name T && git config user.email t@t &&
	echo a >a.txt && echo b >b.txt &&
	git add a.txt b.txt &&
	git commit -m "all" 2>/dev/null &&
	git status >../actual_allclean &&
	grep "nothing to commit" ../actual_allclean
'

# ── status on new branch ─────────────────────────────────────────────────

test_expect_success 'status shows new branch name' '
	cd repo &&
	git branch newbranch &&
	git checkout newbranch 2>/dev/null &&
	git status >actual &&
	grep "On branch newbranch" actual
'

test_expect_success 'status -s -b shows new branch in short mode' '
	cd repo &&
	git status -s -b >actual &&
	grep "^## newbranch" actual
'

# ── status with only staged new files (no prior commits of that file) ────

test_expect_success 'status new file shows as new file' '
	cd repo &&
	echo fresh >fresh.txt &&
	git add fresh.txt &&
	git status >actual &&
	grep "new file:   fresh.txt" actual
'

# ── porcelain vs long format consistency ──────────────────────────────────

test_expect_success 'porcelain and long format agree on file status' '
	cd repo &&
	git status --porcelain >porcelain &&
	git status >long_status &&
	grep "fresh.txt" porcelain &&
	grep "fresh.txt" long_status
'

# ── status porcelain is stable ─────────────────────────────────────────────

test_expect_success 'porcelain format is stable' '
	git init stable_repo &&
	cd stable_repo &&
	git config user.name T && git config user.email t@t &&
	echo x >x.txt && git add x.txt && git commit -m init 2>/dev/null &&
	echo y >y.txt &&
	git status --porcelain >../run1 &&
	git status --porcelain >../run2 &&
	test_cmp ../run1 ../run2
'

test_expect_success 'short format shows same files as porcelain' '
	cd stable_repo &&
	git status -s >../short_out &&
	grep "y.txt" ../short_out &&
	grep "y.txt" ../run1
'

test_done
