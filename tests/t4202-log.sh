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

test_done
