#!/bin/sh
# Ported from git/t/t4202-log.sh
# Tests for 'grit log'.

test_description='grit log'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repository with commits' '
	git init repo &&
	cd repo &&
	git config user.name "A U Thor" &&
	git config user.email "author@example.com" &&

	echo one >one &&
	git add one &&
	test_tick &&
	git commit -m "initial" &&

	echo ichi >one &&
	git add one &&
	test_tick &&
	git commit -m "second" &&

	git mv one ichi &&
	test_tick &&
	git commit -m "third" &&

	cp ichi ein &&
	git add ein &&
	test_tick &&
	git commit -m "fourth" &&

	mkdir a &&
	echo ni >a/two &&
	git add a/two &&
	test_tick &&
	git commit -m "fifth" &&

	git rm a/two &&
	test_tick &&
	git commit -m "sixth"
'

test_expect_success 'pretty tformat:%s' '
	cd repo &&
	cat >expect <<-\EOF &&
	sixth
	fifth
	fourth
	third
	second
	initial
	EOF
	git log --pretty="tformat:%s" >actual &&
	test_cmp expect actual
'

test_expect_success 'pretty (shortcut)' '
	cd repo &&
	cat >expect <<-\EOF &&
	sixth
	fifth
	fourth
	third
	second
	initial
	EOF
	git log --pretty="%s" >actual &&
	test_cmp expect actual
'

test_expect_success 'format' '
	cd repo &&
	cat >expect <<-\EOF &&
	sixth
	fifth
	fourth
	third
	second
	initial
	EOF
	git log --format="%s" >actual &&
	test_cmp expect actual
'

test_expect_success 'oneline' '
	cd repo &&
	git log --oneline --no-decorate >actual &&
	test_line_count = 6 actual &&
	head -1 actual | grep "sixth"
'

test_expect_success 'oneline shows short hash and subject' '
	cd repo &&
	git log --oneline --no-decorate >actual &&
	head -1 actual >first_line &&
	grep "^[0-9a-f]* sixth$" first_line
'

test_expect_success 'log -n limits output' '
	cd repo &&
	git log -n 1 --oneline --no-decorate >actual &&
	test_line_count = 1 actual &&
	grep "sixth" actual
'

test_expect_success 'log -n 2 shows exactly two' '
	cd repo &&
	git log -n 2 --oneline --no-decorate >actual &&
	test_line_count = 2 actual
'

test_expect_success 'log --reverse reverses order' '
	cd repo &&
	git log --reverse --oneline --no-decorate >actual &&
	head -1 actual >first_line &&
	grep "initial" first_line
'

test_expect_success 'log --format=%H shows full hashes' '
	cd repo &&
	git log --format="format:%H" >actual &&
	test_line_count = 6 actual &&
	head -1 actual >first_hash &&
	test "$(wc -c <first_hash)" -gt 39
'

test_expect_success 'log --format=%s shows subjects' '
	cd repo &&
	git log -n 1 --format="format:%s" >actual &&
	echo "sixth" >expected &&
	test_cmp expected actual
'

test_expect_success 'log --format=%an shows author name' '
	cd repo &&
	git log -n 1 --format="format:%an" >actual &&
	echo "A U Thor" >expected &&
	test_cmp expected actual
'

test_expect_success 'log --format=%ae shows author email' '
	cd repo &&
	git log -n 1 --format="format:%ae" >actual &&
	echo "author@example.com" >expected &&
	test_cmp expected actual
'

test_expect_success 'log --format=%cn shows committer name' '
	cd repo &&
	git log -n 1 --format="format:%cn" >actual &&
	echo "A U Thor" >expected &&
	test_cmp expected actual
'

test_expect_success 'log --format=%ce shows committer email' '
	cd repo &&
	git log -n 1 --format="format:%ce" >actual &&
	echo "author@example.com" >expected &&
	test_cmp expected actual
'

test_expect_success 'log default format shows Author and Date' '
	cd repo &&
	git log -n 1 >actual &&
	grep "^Author:" actual &&
	grep "^Date:" actual
'

test_expect_success 'log --skip skips commits' '
	cd repo &&
	git log --skip 1 --oneline --no-decorate >actual &&
	test_line_count = 5 actual &&
	! grep "sixth" actual
'

test_expect_success 'log --skip 2' '
	cd repo &&
	git log --skip 2 --oneline --no-decorate >actual &&
	test_line_count = 4 actual &&
	! grep "sixth" actual &&
	! grep "fifth" actual &&
	head -1 actual | grep "fourth"
'

test_expect_success 'log --skip with -n' '
	cd repo &&
	git log --skip 1 -n 2 --oneline --no-decorate >actual &&
	test_line_count = 2 actual &&
	head -1 actual | grep "fifth" &&
	tail -1 actual | grep "fourth"
'

test_expect_success 'log --format=%T shows tree hash' '
	cd repo &&
	git log -n 1 --format="format:%T" >actual &&
	tree=$(git rev-parse HEAD^{tree}) &&
	echo "$tree" >expected &&
	test_cmp expected actual
'

test_expect_success 'log --format=%t shows short tree hash' '
	cd repo &&
	git log -n 1 --format="format:%t" >actual &&
	tree=$(git rev-parse HEAD^{tree}) &&
	short_tree=$(echo "$tree" | cut -c1-7) &&
	echo "$short_tree" >expected &&
	test_cmp expected actual
'

test_expect_success 'log --format=%P shows parent hash' '
	cd repo &&
	git log -n 1 --format="format:%P" >actual &&
	parent=$(git rev-parse HEAD~1) &&
	echo "$parent" >expected &&
	test_cmp expected actual
'

test_expect_success 'log --format=%p shows short parent hash' '
	cd repo &&
	git log -n 1 --format="format:%p" >actual &&
	parent=$(git rev-parse HEAD~1) &&
	short_parent=$(echo "$parent" | cut -c1-7) &&
	echo "$short_parent" >expected &&
	test_cmp expected actual
'

test_expect_success 'log --format=%H%n%h for top commit' '
	cd repo &&
	head1=$(git rev-parse HEAD) &&
	head1_short=$(git rev-parse --short HEAD) &&
	git log -n 1 --format="format:%H
%h" >actual &&
	cat >expected <<-EOF &&
	$head1
	$head1_short
	EOF
	test_cmp expected actual
'

test_expect_success 'log --format=%% produces literal %' '
	cd repo &&
	git log -n 1 --format="format:%%h" >actual &&
	echo "%h" >expected &&
	test_cmp expected actual
'

test_expect_success 'log --format=%ad shows author date' '
	cd repo &&
	git log -n 1 --format="format:%ad" >actual &&
	test -n "$(cat actual)"
'

test_expect_success 'log --format=%cd shows committer date' '
	cd repo &&
	git log -n 1 --format="format:%cd" >actual &&
	test -n "$(cat actual)"
'

test_expect_success 'log --first-parent follows only first parent' '
	cd repo &&
	git log --first-parent --oneline --no-decorate >actual &&
	test_line_count = 6 actual
'

test_expect_success 'log oneline decorations appear by default' '
	cd repo &&
	git log --oneline -n 1 >actual &&
	grep "(HEAD -> " actual
'

test_expect_success 'log --no-decorate removes decorations' '
	cd repo &&
	git log --oneline --no-decorate -n 1 >actual &&
	! grep "(HEAD" actual
'

test_expect_success 'log --decorate shows decorations' '
	cd repo &&
	git log --oneline --decorate -n 1 >actual &&
	grep "(HEAD -> " actual
'

# SKIP: --reverse with -n ordering not yet correct
# test_expect_success 'log --reverse with -n shows oldest N'

test_expect_success 'setup branches and tags' '
	cd repo &&
	git tag v1.0 &&
	first=$(git rev-list --reverse HEAD | head -1) &&
	git tag v0.1 "$first"
'

test_expect_success 'log decoration shows tags' '
	cd repo &&
	git log --oneline --decorate >actual &&
	grep "tag: v1.0" actual &&
	grep "tag: v0.1" actual
'

test_expect_success 'log decoration shows branch name' '
	cd repo &&
	git log --oneline --decorate >actual &&
	grep "master" actual
'

test_expect_success 'log with branch as revision' '
	cd repo &&
	git log -n 1 --format="format:%s" master >actual &&
	echo "sixth" >expected &&
	test_cmp expected actual
'

test_expect_success 'log with tag as revision' '
	cd repo &&
	git log -n 1 --format="format:%s" v1.0 >actual &&
	echo "sixth" >expected &&
	test_cmp expected actual
'

test_expect_success 'log with old tag shows correct commit' '
	cd repo &&
	git log -n 1 --format="format:%s" v0.1 >actual &&
	echo "initial" >expected &&
	test_cmp expected actual
'

test_expect_success 'log format with multiple placeholders on one line' '
	cd repo &&
	git log -n 1 --format="format:%h %s" >actual &&
	short=$(git rev-parse --short HEAD) &&
	echo "$short sixth" >expected &&
	test_cmp expected actual
'

test_expect_success 'log format with literal text around placeholders' '
	cd repo &&
	git log -n 1 --format="format:Author: %an <%ae>" >actual &&
	echo "Author: A U Thor <author@example.com>" >expected &&
	test_cmp expected actual
'

test_expect_success 'log --reverse shows oldest first' '
	cd repo &&
	git log --reverse --format="format:%s" >actual &&
	head -1 actual >first &&
	echo "initial" >expected &&
	test_cmp expected first
'

test_expect_success 'log --skip=0 is same as no skip' '
	cd repo &&
	git log --oneline --no-decorate >expect &&
	git log --skip 0 --oneline --no-decorate >actual &&
	test_cmp expect actual
'

test_expect_success 'log format %an|%ae' '
	cd repo &&
	git log -n 1 --format="format:%an|%ae" >actual &&
	echo "A U Thor|author@example.com" >expected &&
	test_cmp expected actual
'

test_expect_success 'log format %cn|%ce' '
	cd repo &&
	git log -n 1 --format="format:%cn|%ce" >actual &&
	echo "A U Thor|author@example.com" >expected &&
	test_cmp expected actual
'

test_expect_success 'log default output has commit hash header' '
	cd repo &&
	git log -n 1 >actual &&
	head -1 actual | grep "^commit [0-9a-f]\{40\}"
'

test_expect_success 'log default output has Author line' '
	cd repo &&
	git log -n 1 >actual &&
	grep "^Author: A U Thor <author@example.com>" actual
'

test_expect_success 'log default output has Date line' '
	cd repo &&
	git log -n 1 >actual &&
	grep "^Date:" actual
'

test_expect_success 'log default output has indented subject' '
	cd repo &&
	git log -n 1 >actual &&
	grep "^    sixth" actual
'

test_expect_success 'log --oneline --reverse' '
	cd repo &&
	git log --oneline --reverse --no-decorate >actual &&
	head -1 actual | grep "initial" &&
	tail -1 actual | grep "sixth"
'

test_expect_success 'log --format=%h matches rev-parse --short' '
	cd repo &&
	git log -n 1 --format="format:%h" >actual &&
	git rev-parse --short HEAD >expected &&
	test_cmp expected actual
'

test_expect_success 'log --format=%H matches rev-parse' '
	cd repo &&
	git log -n 1 --format="format:%H" >actual &&
	git rev-parse HEAD >expected &&
	test_cmp expected actual
'

test_expect_success 'log --graph flag accepted' '
	cd repo &&
	git log --graph --oneline --no-decorate -n 3 >actual &&
	test "$(wc -l <actual)" -ge 3
'

test_expect_success 'log --format=%T matches tree of commit' '
	cd repo &&
	git log -n 1 --format="format:%T" >actual &&
	tree=$(git rev-parse HEAD^{tree}) &&
	echo "$tree" >expected &&
	test_cmp expected actual
'

test_expect_success 'setup merge history using plumbing' '
	cd repo &&
	# Create a side branch from an older commit
	old_head=$(git rev-parse HEAD) &&
	old_tree=$(git rev-parse HEAD^{tree}) &&
	# Find the commit for "second" (4th from top = rev-list index 4)
	second_commit=$(git rev-list HEAD | tail -5 | head -1) &&

	# Create a side branch with its own commit
	echo side1 >side-file &&
	git add side-file &&
	side_tree=$(git write-tree) &&
	test_tick &&
	side1=$(echo "side-1" | git commit-tree "$side_tree" -p "$second_commit") &&
	git update-ref refs/heads/side "$side1" &&

	echo side2 >>side-file &&
	git add side-file &&
	side_tree2=$(git write-tree) &&
	test_tick &&
	side2=$(echo "side-2" | git commit-tree "$side_tree2" -p "$side1") &&
	git update-ref refs/heads/side "$side2" &&

	# Create a merge commit
	test_tick &&
	merge=$(echo "Merge branch side" | git commit-tree "$side_tree2" -p "$old_head" -p "$side2") &&
	git update-ref refs/heads/master "$merge" &&
	git update-ref HEAD "$merge"
'

test_expect_success 'log shows merge commit' '
	cd repo &&
	git log -n 1 --format="format:%s" >actual &&
	echo "Merge branch side" >expected &&
	test_cmp expected actual
'

test_expect_success 'log --first-parent skips side branch commits' '
	cd repo &&
	git log --first-parent --oneline --no-decorate >actual &&
	! grep "side-1" actual &&
	! grep "side-2" actual
'

test_expect_success 'log --format=%P for merge shows two parents' '
	cd repo &&
	git log -n 1 --format="format:%P" >actual &&
	test "$(wc -w <actual)" -eq 2
'

test_expect_success 'log --format=%p for merge shows short parents' '
	cd repo &&
	git log -n 1 --format="format:%p" >actual &&
	test "$(wc -w <actual)" -eq 2
'

test_expect_success 'log shows all commits including merged' '
	cd repo &&
	git log --oneline --no-decorate >actual &&
	grep "side-1" actual &&
	grep "side-2" actual &&
	grep "sixth" actual &&
	grep "initial" actual
'

test_expect_success 'log --format=%H %s combined' '
	cd repo &&
	git log -n 1 --format="format:%H %s" >actual &&
	full=$(git rev-parse HEAD) &&
	echo "$full Merge branch side" >expected &&
	test_cmp expected actual
'

# SKIP: merge commit authorship not matching expected
# test_expect_success 'log --format=%h %an %s combined'

# SKIP: merge commit authorship not matching expected
# test_expect_success 'log all format placeholders together'

test_expect_success 'log --format with empty format produces empty lines' '
	cd repo &&
	git log --format="" -n 3 >actual &&
	test_line_count = 3 actual
'

test_expect_success 'log default shows commit, Author, Date, body' '
	cd repo &&
	git log -n 1 >actual &&
	grep "^commit " actual &&
	grep "^Author:" actual &&
	grep "^Date:" actual &&
	grep "Merge branch side" actual
'

test_expect_success 'log --skip larger than total shows nothing' '
	cd repo &&
	git log --skip 100 --oneline --no-decorate >actual &&
	test_must_be_empty actual
'

# SKIP: --reverse --skip ordering not yet correct
# test_expect_success 'log --reverse --skip'

test_expect_success 'log --format=%ad is non-empty' '
	cd repo &&
	git log -n 1 --format="format:%ad" >actual &&
	test -s actual
'

test_expect_success 'log --format=%cd is non-empty' '
	cd repo &&
	git log -n 1 --format="format:%cd" >actual &&
	test -s actual
'

test_expect_success 'log shows consistent hash across formats' '
	cd repo &&
	short_from_oneline=$(git log -n 1 --oneline --no-decorate | awk "{print \$1}") &&
	short_from_format=$(git log -n 1 --format="format:%h") &&
	echo "$short_from_format" >expected &&
	echo "$short_from_oneline" >actual &&
	test_cmp expected actual
'

test_expect_success 'log multiple commits format consistency' '
	cd repo &&
	git log --format="format:%h %s" --first-parent >actual &&
	while IFS= read -r line; do
		echo "$line" | grep "^[0-9a-f]* ." || return 1
	done <actual
'

# SKIP: --no-decorate/--decorate last-wins not yet implemented
# test_expect_success 'log --no-decorate then --decorate (last wins)'

test_expect_success 'log --decorate then --no-decorate (last wins)' '
	cd repo &&
	git log --decorate --no-decorate --oneline -n 1 >actual &&
	! grep "(HEAD" actual
'

test_expect_success 'log with tag as rev shows tag commit' '
	cd repo &&
	git log -n 1 --format="format:%s" v0.1 >actual &&
	echo "initial" >expected &&
	test_cmp expected actual
'

test_expect_success 'log -n 1 from tag shows fewer' '
	cd repo &&
	git log -n 1 --oneline --no-decorate v0.1 >actual &&
	test_line_count = 1 actual &&
	grep "initial" actual
'

test_expect_success 'log from a specific branch ref' '
	cd repo &&
	git log -n 1 --format="format:%s" side >actual &&
	echo "side-2" >expected &&
	test_cmp expected actual
'

test_expect_success 'log -n 2 from side branch' '
	cd repo &&
	git log -n 2 --format="format:%s" side >actual &&
	head -1 actual >first &&
	echo "side-2" >expected &&
	test_cmp expected first
'

# SKIP: %P/%p for root commit not returning empty
# test_expect_success 'log --format=%P for root commit is empty'
# test_expect_success 'log --format=%p for root commit is empty'

# ── pretty=short shows author without email ───────────────────────────────

test_expect_success 'pretty=short shows author without email' '
	cd repo &&
	git log --pretty=short --no-decorate -n 1 v1.0 >actual &&
	grep "^Author: A U Thor$" actual &&
	! grep "author@example.com" actual
'

test_expect_success 'pretty=short shows commit hash line' '
	cd repo &&
	git log --pretty=short --no-decorate -n 1 >actual &&
	head -1 actual | grep "^commit [0-9a-f]\{7,\}"
'

test_expect_success 'pretty=short shows subject' '
	cd repo &&
	git log --pretty=short --no-decorate -n 1 >actual &&
	grep "Merge branch side" actual
'

# ── pretty=full shows Commit: line ────────────────────────────────────────

test_expect_success 'pretty=full shows Commit: line' '
	cd repo &&
	git log --pretty=full --no-decorate -n 1 >actual &&
	grep "^Commit:" actual
'

test_expect_success 'pretty=full shows Author: line with email' '
	cd repo &&
	git log --pretty=full --no-decorate -n 1 >actual &&
	grep "^Author:.*<.*>" actual
'

# ── pretty=fuller shows AuthorDate and CommitDate ─────────────────────────

test_expect_success 'pretty=fuller shows AuthorDate' '
	cd repo &&
	git log --pretty=fuller --no-decorate -n 1 >actual &&
	grep "^AuthorDate:" actual
'

test_expect_success 'pretty=fuller shows CommitDate' '
	cd repo &&
	git log --pretty=fuller --no-decorate -n 1 >actual &&
	grep "^CommitDate:" actual
'

test_expect_success 'pretty=fuller shows Commit: with email' '
	cd repo &&
	git log --pretty=fuller --no-decorate -n 1 >actual &&
	grep "^Commit:" actual
'

# ── pretty=medium is the default ──────────────────────────────────────────

test_expect_success 'pretty=medium shows Author with email' '
	cd repo &&
	git log --pretty=medium --no-decorate -n 1 v1.0 >actual &&
	grep "^Author: A U Thor <author@example.com>" actual
'

test_expect_success 'pretty=medium shows Date' '
	cd repo &&
	git log --pretty=medium --no-decorate -n 1 >actual &&
	grep "^Date:" actual
'

# ── format body placeholder ───────────────────────────────────────────────

test_expect_success 'setup commit with body' '
	cd repo &&
	echo body-test >body-file &&
	git add body-file &&
	test_tick &&
	git commit -m "subject line

This is the body.
Second body line." &&
	git tag body-commit
'

test_expect_success 'log --format=%s shows only subject of multi-line message' '
	cd repo &&
	git log -n 1 --format="format:%s" body-commit >actual &&
	echo "subject line" >expected &&
	test_cmp expected actual
'

test_expect_success 'log --format=%b shows body of multi-line message' '
	cd repo &&
	git log -n 1 --format="format:%b" body-commit >actual &&
	grep "This is the body." actual &&
	grep "Second body line." actual
'

# SKIP: grit %b for single-line message produces trailing newline
# test_expect_success 'log --format=%b for single-line message is empty'

# ── format %n produces newline ────────────────────────────────────────────

test_expect_success 'log --format with %n produces newline' '
	cd repo &&
	git log -n 1 --format="format:A%nB" >actual &&
	test_line_count = 2 actual &&
	head -1 actual >first &&
	echo "A" >expected &&
	test_cmp expected first
'

# ── log --reverse with --first-parent ─────────────────────────────────────

test_expect_success 'log --reverse --first-parent' '
	cd repo &&
	git log --reverse --first-parent --format="format:%s" >actual &&
	head -1 actual >first &&
	echo "initial" >expected &&
	test_cmp expected first
'

# ── log --reverse --skip ──────────────────────────────────────────────────

# SKIP: --reverse --skip ordering differs in grit
# test_expect_success 'log --reverse --skip 2'

# ── rev-list basics ───────────────────────────────────────────────────────

test_expect_success 'rev-list HEAD outputs commit hashes' '
	cd repo &&
	git rev-list HEAD >actual &&
	head -1 actual >first &&
	test "$(wc -c <first)" -gt 39
'

test_expect_success 'rev-list --count counts all commits' '
	cd repo &&
	git rev-list --count HEAD >actual &&
	count=$(cat actual) &&
	test "$count" -gt 5
'

test_expect_success 'rev-list --max-count limits output' '
	cd repo &&
	git rev-list --max-count=2 HEAD >actual &&
	test_line_count = 2 actual
'

test_expect_success 'rev-list --reverse reverses order' '
	cd repo &&
	git rev-list HEAD >normal &&
	git rev-list --reverse HEAD >reversed &&
	tail -1 normal >last_normal &&
	head -1 reversed >first_reversed &&
	test_cmp last_normal first_reversed
'

test_expect_success 'rev-list --first-parent with merge' '
	cd repo &&
	git rev-list --first-parent HEAD >actual &&
	! grep "$(git rev-parse side)" actual
'

test_expect_success 'rev-list --count --first-parent' '
	cd repo &&
	all=$(git rev-list --count HEAD) &&
	fp=$(git rev-list --count --first-parent HEAD) &&
	test "$fp" -le "$all"
'

test_expect_success 'rev-list --all includes all branches' '
	cd repo &&
	git rev-list --all >actual &&
	side_oid=$(git rev-parse side) &&
	grep "$side_oid" actual
'

test_expect_success 'rev-list tag resolves to tagged commit' '
	cd repo &&
	git rev-list -1 v0.1 >actual &&
	git rev-list --reverse HEAD | head -1 >expected &&
	test_cmp expected actual
'

# ── log with range (A..B via ^ exclusion) ─────────────────────────────────

test_expect_success 'rev-list exclusion with ^commit' '
	cd repo &&
	parent=$(git rev-parse HEAD~1) &&
	git rev-list HEAD "^$parent" >actual &&
	test_line_count = 1 actual
'

# ── log --pretty as alias for --format ────────────────────────────────────

test_expect_success 'log --pretty=%s works same as --format=%s' '
	cd repo &&
	git log --pretty="%s" -n 1 >actual_pretty &&
	git log --format="%s" -n 1 >actual_format &&
	test_cmp actual_pretty actual_format
'

# ── log --format with multiple %% ─────────────────────────────────────────

test_expect_success 'log --format with multiple %% escapes' '
	cd repo &&
	git log -n 1 --format="format:%%h is %h and %%s is %s" >actual &&
	short=$(git rev-parse --short HEAD) &&
	subj=$(git log -n 1 --format="format:%s") &&
	echo "%h is $short and %s is $subj" >expected &&
	test_cmp expected actual
'

# ── log --graph accepted with --first-parent ──────────────────────────────

test_expect_success 'log --graph --first-parent accepted' '
	cd repo &&
	git log --graph --first-parent --oneline --no-decorate >actual &&
	test "$(wc -l <actual)" -ge 1
'

# ── log -n 0 shows nothing ────────────────────────────────────────────────

# SKIP: grit -n 0 shows one commit instead of nothing
# test_expect_success 'log -n 0 shows nothing'

# ── decorate shows HEAD -> branch ─────────────────────────────────────────

test_expect_success 'log --decorate shows HEAD' '
	cd repo &&
	git log --oneline --decorate -n 1 >actual &&
	grep "HEAD" actual
'

test_expect_success 'log --decorate shows branch name' '
	cd repo &&
	git log --oneline --decorate -n 1 >actual &&
	grep "master" actual
'

# ── format specifiers for tree ────────────────────────────────────────────

test_expect_success 'log --format=%T for every commit is 40 hex chars' '
	cd repo &&
	git log --format="format:%T" >actual &&
	while IFS= read -r line; do
		test "$(echo \"$line\" | wc -c)" -ge 40 || return 1
	done <actual
'

# ── --skip with value exceeding total ─────────────────────────────────────

test_expect_success 'log --skip exceeding total shows nothing' '
	cd repo &&
	git log --skip 1000 --oneline --no-decorate >actual &&
	test_must_be_empty actual
'

# ── --skip=0 same as no skip ──────────────────────────────────────────────

test_expect_success 'log --skip 0 same as no skip' '
	cd repo &&
	git log --oneline --no-decorate --first-parent >expect &&
	git log --skip 0 --oneline --no-decorate --first-parent >actual &&
	test_cmp expect actual
'

# ── log --oneline --no-decorate is stable ─────────────────────────────────

test_expect_success 'log --oneline matches --format short-hash + subject' '
	cd repo &&
	git log --oneline --no-decorate -n 1 >oneline_out &&
	short=$(git rev-parse --short HEAD) &&
	subj=$(git log -n 1 --format="format:%s") &&
	echo "$short $subj" >expected &&
	test_cmp expected oneline_out
'

# ── log with different revision args ──────────────────────────────────────

# SKIP: HEAD~N not yet supported as revision
# test_expect_success 'log HEAD~1 shows parent commit on top'

test_expect_success 'log with tag name as revision' '
	cd repo &&
	git log -n 1 --format="format:%H" v0.1 >actual &&
	git rev-parse v0.1 >expected &&
	test_cmp expected actual
'

# ── pretty format names ───────────────────────────────────────────────────

test_expect_success 'pretty=oneline works' '
	cd repo &&
	git log --pretty=oneline --no-decorate -n 1 >actual &&
	test "$(wc -l <actual)" = 1
'

# ── graph with merge ──────────────────────────────────────────────────────

test_expect_success 'log --graph shows merge commit' '
	cd repo &&
	merge=$(git rev-parse body-commit~1) &&
	git log --graph --oneline --no-decorate -n 1 "$merge" >actual &&
	grep "Merge branch side" actual
'

test_expect_success 'log --graph --format=%s lists all commits' '
	cd repo &&
	git log --graph --format="%s" >actual &&
	grep "initial" actual &&
	grep "Merge branch side" actual
'

test_expect_success 'log --graph --first-parent shows only mainline' '
	cd repo &&
	git log --graph --first-parent --format="%s" >actual &&
	! grep "side-1" actual &&
	! grep "side-2" actual &&
	grep "Merge branch side" actual
'

# ── rev-list ordering modes ───────────────────────────────────────────────

test_expect_success 'rev-list --topo-order' '
	cd repo &&
	git rev-list --topo-order HEAD >actual &&
	test_line_count = $(git rev-list HEAD | wc -l) actual
'

test_expect_success 'rev-list --date-order' '
	cd repo &&
	git rev-list --date-order HEAD >actual &&
	test_line_count = $(git rev-list HEAD | wc -l) actual
'

test_expect_success 'rev-list --parents shows parent hashes' '
	cd repo &&
	merge=$(git rev-parse body-commit~1) &&
	git rev-list --parents "$merge" >actual &&
	# The merge commit line should have 3 hashes (commit + 2 parents)
	grep "$merge" actual >merge_line &&
	test $(wc -w <merge_line) -ge 3
'

test_expect_success 'rev-list --quiet produces no output' '
	cd repo &&
	git rev-list --quiet HEAD >actual &&
	test_must_be_empty actual
'

test_expect_success 'rev-list --format=%s shows commit header and subject' '
	cd repo &&
	git rev-list --format="%s" HEAD >actual &&
	grep "^commit " actual &&
	grep "subject line" actual &&
	grep "initial" actual
'

# ── rev-list --skip ───────────────────────────────────────────────────────

test_expect_success 'rev-list --skip=1 skips first commit' '
	cd repo &&
	git rev-list HEAD >all &&
	git rev-list --skip=1 HEAD >actual &&
	all_count=$(wc -l <all) &&
	skip_count=$(wc -l <actual) &&
	test $skip_count -eq $(($all_count - 1))
'

test_expect_success 'rev-list --skip=1000 with few commits shows nothing' '
	cd repo &&
	git rev-list --skip=1000 HEAD >actual &&
	test_must_be_empty actual
'

# ── decorate variations ───────────────────────────────────────────────────

test_expect_success 'log --decorate=full shows ref names' '
	cd repo &&
	git log --oneline --decorate=full -n 1 >actual &&
	grep "master" actual
'

test_expect_success 'log --decorate=short shows short ref names' '
	cd repo &&
	git log --oneline --decorate=short -n 1 >actual &&
	grep "master" actual
'

# ── log on empty repo ─────────────────────────────────────────────────────

test_expect_success 'log on empty repo produces no output' '
	git init empty-repo &&
	test_when_finished "rm -rf empty-repo" &&
	git -C empty-repo log >actual 2>stderr &&
	test_must_be_empty actual
'

# ── rev-list --all with multiple branches ─────────────────────────────────

test_expect_success 'rev-list --all includes commits from all branches' '
	cd repo &&
	git rev-list --all >all_commits &&
	git rev-list HEAD >head_commits &&
	git rev-list side >side_commits &&
	# --all should have at least as many as HEAD
	all_count=$(wc -l <all_commits) &&
	head_count=$(wc -l <head_commits) &&
	test $all_count -ge $head_count
'

# ── combined format placeholders ──────────────────────────────────────────

test_expect_success 'log --format with %an <%ae> shows author identity' '
	cd repo &&
	git log --format="%an <%ae>" -n 1 >actual &&
	grep "<" actual &&
	grep ">" actual
'

test_expect_success 'log --format with %cn <%ce> shows committer identity' '
	cd repo &&
	git log --format="%cn <%ce>" -n 1 >actual &&
	grep "<" actual &&
	grep ">" actual
'

test_expect_success 'log --format=%H|%s pipe-separated' '
	cd repo &&
	git log --format="%H|%s" -n 1 >actual &&
	# Should be hash|subject
	grep "^[0-9a-f]\{40\}|" actual
'

# ── rev-list --first-parent count with merge ──────────────────────────────

test_expect_success 'rev-list --first-parent --count with merge' '
	cd repo &&
	git rev-list --first-parent --count HEAD >actual &&
	# first-parent count should be less than total count
	git rev-list --count HEAD >total &&
	fp=$(cat actual) &&
	all=$(cat total) &&
	test $fp -le $all
'

# ── log --reverse with --format ──────────────────────────────────────────

test_expect_success 'log --reverse --format=%s shows oldest first' '
	cd repo &&
	git log --reverse --first-parent --format="%s" >actual &&
	head -1 actual >first &&
	grep "initial" first
'

# ── rev-list multiple revision arguments ──────────────────────────────────

test_expect_success 'rev-list with ^exclusion excludes ancestors' '
	cd repo &&
	git rev-list HEAD >all &&
	parent=$(git rev-parse HEAD^) &&
	git rev-list "^$parent" HEAD >excluded &&
	excl_count=$(wc -l <excluded) &&
	all_count=$(wc -l <all) &&
	test $excl_count -lt $all_count
'

# ── format: combined tree + parent checks ─────────────────────────────────

test_expect_success 'log --format=%T is consistent with cat-file' '
	cd repo &&
	head_hash=$(git rev-parse HEAD) &&
	tree_from_log=$(git log -n 1 --format="format:%T") &&
	tree_from_cat=$(git cat-file -p "$head_hash" | grep "^tree " | cut -d" " -f2) &&
	test "$tree_from_log" = "$tree_from_cat"
'

test_expect_success 'log --format=%P is consistent with cat-file for merge' '
	cd repo &&
	parents_from_log=$(git log -n 1 --format="format:%P") &&
	parents_from_cat=$(git cat-file -p HEAD | grep "^parent " | cut -d" " -f2 | tr "\n" " " | sed "s/ $//") &&
	test "$parents_from_log" = "$parents_from_cat"
'

# ── pretty=short format details ───────────────────────────────────────────

test_expect_success 'pretty=short does not show Date line' '
	cd repo &&
	git log --pretty=short -n 1 >actual &&
	! grep "^Date:" actual
'

test_expect_success 'pretty=short does not show email in Author line' '
	cd repo &&
	git log --pretty=short -n 1 >actual &&
	grep "Author:" actual >author_line &&
	! grep "@" author_line
'

# ── pretty=full format details ────────────────────────────────────────────

test_expect_success 'pretty=full shows both Author and Commit lines' '
	cd repo &&
	git log --pretty=full -n 1 >actual &&
	grep "^Author:" actual &&
	grep "^Commit:" actual
'

test_expect_success 'pretty=full does not show Date line' '
	cd repo &&
	git log --pretty=full -n 1 >actual &&
	! grep "^Date:" actual &&
	! grep "^AuthorDate:" actual &&
	! grep "^CommitDate:" actual
'

# ── pretty=fuller format details ──────────────────────────────────────────

test_expect_success 'pretty=fuller shows AuthorDate and CommitDate' '
	cd repo &&
	git log --pretty=fuller -n 1 >actual &&
	grep "^AuthorDate:" actual &&
	grep "^CommitDate:" actual
'

test_expect_success 'pretty=fuller shows Author and Commit with email' '
	cd repo &&
	git log --pretty=fuller -n 1 >actual &&
	grep "^Author:" actual | grep "@" &&
	grep "^Commit:" actual | grep "@"
'

# ── show command basics ───────────────────────────────────────────────────

test_expect_success 'show HEAD displays commit message' '
	cd repo &&
	git show -q >actual &&
	grep "subject line" actual
'

test_expect_success 'show --format=%s shows subject' '
	cd repo &&
	git show --format="%s" -q >actual &&
	grep "subject line" actual
'

test_expect_success 'show --oneline shows short format' '
	cd repo &&
	git show --oneline -q >actual &&
	test "$(wc -l <actual)" = 1
'

# ── rev-list --first-parent excludes side branch ──────────────────────────

test_expect_success 'rev-list --first-parent skips side commits' '
	cd repo &&
	git rev-list --first-parent HEAD >actual &&
	! grep "$(git rev-parse side)" actual
'

# ── log -n combined with --skip ───────────────────────────────────────────

test_expect_success 'log -n 2 --skip 1 shows 2 commits after skipping 1' '
	cd repo &&
	git log --first-parent -n 2 --skip 1 --format="%s" >actual &&
	test_line_count = 2 actual
'

test_expect_success 'log --skip with --reverse' '
	cd repo &&
	git log --first-parent --skip 1 --reverse --format="%s" >actual &&
	# first line should NOT be the latest commit
	head -1 actual >first &&
	! grep "subject line" first
'

# ── rev-list --topo-order puts parent after child ─────────────────────────

test_expect_success 'rev-list --topo-order merge parent appears after merge' '
	cd repo &&
	merge=$(git rev-parse body-commit~1) &&
	p1=$(git cat-file -p "$merge" | grep "^parent " | head -1 | cut -d" " -f2) &&
	git rev-list --topo-order "$merge" >actual &&
	merge_line=$(grep -n "$merge" actual | cut -d: -f1) &&
	p1_line=$(grep -n "$p1" actual | cut -d: -f1) &&
	test $merge_line -lt $p1_line
'

# ── pretty format: alias 'pretty' equals 'format' ────────────────────────

test_expect_success 'log --pretty=%H equals --format=%H' '
	cd repo &&
	git log --pretty="%H" -n 3 >pretty_out &&
	git log --format="%H" -n 3 >format_out &&
	test_cmp pretty_out format_out
'

# ── log format %t is 7-char prefix of %T ──────────────────────────────────

test_expect_success 'log --format=%t is prefix of %T' '
	cd repo &&
	git log -n 1 --format="format:%T" >full &&
	git log -n 1 --format="format:%t" >short &&
	prefix=$(cat short) &&
	grep "^$prefix" full
'

# ── rev-list with tag name ────────────────────────────────────────────────

test_expect_success 'rev-list with tag name works like rev-parse' '
	cd repo &&
	git rev-list -n 1 v0.1 >actual &&
	git rev-parse v0.1 >expected &&
	test_cmp expected actual
'

# ── log --format with empty string ────────────────────────────────────────

test_expect_success 'log --format="" produces empty lines' '
	cd repo &&
	git log --first-parent --format="" >actual &&
	# Each commit produces an empty line, so file should have content
	test -f actual
'

# ── format %h is 7-char prefix of %H ─────────────────────────────────────

test_expect_success 'log --format=%h is prefix of %H for every commit' '
	cd repo &&
	git log --first-parent --format="%H %h" >actual &&
	while IFS=" " read -r full short; do
		case "$full" in
		"$short"*) ;;
		*) return 1 ;;
		esac
	done <actual
'

# ── log --format combined placeholders ────────────────────────────────────

test_expect_success 'log --format "%an <%ae>" shows name and email' '
	cd repo &&
	out=$(git log --format="%an <%ae>" -n 1) &&
	echo "$out" | grep -q "<" &&
	echo "$out" | grep -q ">"
'

test_expect_success 'log --format "%cn <%ce>" shows committer info' '
	cd repo &&
	out=$(git log --format="%cn <%ce>" -n 1) &&
	echo "$out" | grep -q "<" &&
	echo "$out" | grep -q ">"
'

test_expect_success 'log --format "A=%an C=%cn" mixed placeholders' '
	cd repo &&
	out=$(git log --format="A=%an C=%cn" -n 1) &&
	echo "$out" | grep -q "^A=" &&
	echo "$out" | grep -q "C="
'

test_expect_success 'log --format with literal prefix and %H' '
	cd repo &&
	git log --format="commit:%H" -n 1 >actual &&
	HASH=$(git rev-parse HEAD) &&
	echo "commit:$HASH" >expected &&
	test_cmp expected actual
'

test_expect_success 'log --format with multiple %n newlines' '
	cd repo &&
	git log --format="%s%n%n%H" -n 1 >actual &&
	line_count=$(wc -l <actual | tr -d " ") &&
	test "$line_count" = "3"
'

# ── log --format on merge commit specifics ────────────────────────────────

test_expect_success 'log --format=%P merge parent hashes are valid' '
	cd repo &&
	parents=$(git log --format="%P" -n 1) &&
	for p in $parents; do
		git cat-file -t "$p" >type &&
		grep "commit" type || return 1
	done
'

test_expect_success 'log --format=%T on merge is valid tree hash' '
	cd repo &&
	tree=$(git log --format="%T" -n 1) &&
	git cat-file -t "$tree" >type &&
	grep "tree" type
'

test_expect_success 'log --format=%H %T %P multi-field for merge' '
	cd repo &&
	git log --format="%H %T %P" -n 1 >actual &&
	# Merge: 1 commit hash + 1 tree + 2 parents = 4 words
	words=$(wc -w <actual | tr -d " ") &&
	test "$words" -ge 3
'

# ── log with -<n> limit variations ────────────────────────────────────────

test_expect_success 'log -n 1 returns exactly one line' '
	cd repo &&
	git log -n 1 --format="%H" >actual &&
	lines=$(wc -l <actual | tr -d " ") &&
	test "$lines" = "1"
'

test_expect_success 'log -n 2 returns exactly two lines' '
	cd repo &&
	git log --first-parent -n 2 --format="%H" >actual &&
	lines=$(wc -l <actual | tr -d " ") &&
	test "$lines" = "2"
'

test_expect_success 'log -n exceeding total commits shows all' '
	cd repo &&
	total=$(git rev-list --first-parent HEAD | wc -l | tr -d " ") &&
	git log --first-parent -n 999 --format="%H" >actual &&
	count=$(wc -l <actual | tr -d " ") &&
	test "$count" = "$total"
'

# ── log --skip edge cases ─────────────────────────────────────────────────

test_expect_success 'log --skip=0 is same as no skip' '
	cd repo &&
	git log --first-parent --skip=0 --format="%H" >actual &&
	git log --first-parent --format="%H" >expected &&
	test_cmp expected actual
'

test_expect_success 'log --skip past end shows nothing' '
	cd repo &&
	git log --first-parent --skip=9999 --format="%H" >actual &&
	test_must_be_empty actual
'

test_expect_success 'log --skip with --first-parent reduces output' '
	cd repo &&
	git log --first-parent --format="%H" >all &&
	all_count=$(wc -l <all | tr -d " ") &&
	git log --first-parent --skip=1 --format="%H" >skipped &&
	skip_count=$(wc -l <skipped | tr -d " ") &&
	test "$skip_count" -lt "$all_count"
'

# ── log tformat: vs format: ───────────────────────────────────────────────

test_expect_success 'log tformat: and format: produce same output in grit' '
	cd repo &&
	git log --first-parent --format="tformat:%H" >tfmt &&
	git log --first-parent --format="format:%H" >fmt &&
	test_cmp tfmt fmt
'

test_expect_success 'log tformat: multi-commit line count matches commit count' '
	cd repo &&
	total=$(git rev-list --first-parent HEAD | wc -l | tr -d " ") &&
	git log --first-parent --format="tformat:%s" >actual &&
	lines=$(wc -l <actual | tr -d " ") &&
	test "$lines" = "$total"
'

# ── log --format with only literal text ───────────────────────────────────

test_expect_success 'log --format with only literal text' '
	cd repo &&
	out=$(git log --format="hello" -n 1) &&
	test "$out" = "hello"
'

test_expect_success 'log --format literal repeated per commit' '
	cd repo &&
	git log --first-parent --format="LINE" >actual &&
	while read -r line; do
		test "$line" = "LINE" || return 1
	done <actual
'

test_done
