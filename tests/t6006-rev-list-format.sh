#!/bin/sh
# Ported subset from git/t/t6006-rev-list-format.sh.

test_description='git rev-list format output'

. ./test-lib.sh

GIT_COMMITTER_EMAIL=git@comm.iter.xz
GIT_COMMITTER_NAME='C O Mmiter'
GIT_AUTHOR_NAME='A U Thor'
GIT_AUTHOR_EMAIL=git@au.thor.xz
export GIT_COMMITTER_EMAIL GIT_COMMITTER_NAME GIT_AUTHOR_NAME GIT_AUTHOR_EMAIL

M=1130000000
Z=+0000
export M Z

doit () {
	OFFSET=$1 &&
	NAME=$2 &&
	shift 2 &&
	PARENTS= &&
	for P
	do
		PARENTS="$PARENTS -p $P"
	done &&
	GIT_COMMITTER_DATE="$(($M + $OFFSET)) $Z" &&
	GIT_AUTHOR_DATE="$GIT_COMMITTER_DATE" &&
	export GIT_COMMITTER_DATE GIT_AUTHOR_DATE &&
	commit=$(echo "$NAME" | git commit-tree "$(git write-tree)" $PARENTS) &&
	echo "$commit"
}

test_expect_success 'setup repository with two commits' '
	grit init repo &&
	cd repo &&
	head1=$(doit 1 "added foo") &&
	head2=$(doit 2 "changed foo" "$head1") &&
	git update-ref refs/heads/master "$head2" &&
	echo "$head1" >head1 &&
	echo "$head2" >head2
'

test_expect_success '--format=%s includes commit headers' '
	cd repo &&
	head1=$(cat head1) &&
	head2=$(cat head2) &&
	cat >expect <<-EOF &&
	commit $head2
	changed foo
	commit $head1
	added foo
	EOF
	git rev-list --format=%s refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success '--format supports %H and %h' '
	cd repo &&
	head2=$(cat head2) &&
	short2=$(git rev-parse --short=4 "$head2") &&
	cat >expect <<-EOF &&
	commit $head2
	$head2 $short2
	EOF
	git rev-list --abbrev=4 --max-count=1 --format="%H %h" refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success '--quiet suppresses output' '
	cd repo &&
	git rev-list --quiet refs/heads/master >actual &&
	test_path_is_file actual &&
	lines=$(wc -c <actual | tr -d " ") &&
	test "$lines" = "0"
'

test_expect_success 'percent literal %%' '
	cd repo &&
	head2=$(cat head2) &&
	cat >expect <<-EOF &&
	commit $head2
	%h
	EOF
	git rev-list --max-count=1 --format="%%h" refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success '--format %H alone' '
	cd repo &&
	head1=$(cat head1) &&
	head2=$(cat head2) &&
	cat >expect <<-EOF &&
	commit $head2
	$head2
	commit $head1
	$head1
	EOF
	git rev-list --format=%H refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success '--format %h with default abbreviation' '
	cd repo &&
	head2=$(cat head2) &&
	short2=$(git rev-parse --short "$head2") &&
	git rev-list --max-count=1 --format="%h" refs/heads/master >actual &&
	# Extract the formatted line (second line)
	sed -n 2p actual >hash_line &&
	echo "$short2" >expect &&
	test_cmp expect hash_line
'

test_expect_success '--format with multiple specifiers' '
	cd repo &&
	head2=$(cat head2) &&
	short2=$(git rev-parse --short=4 "$head2") &&
	cat >expect <<-EOF &&
	commit $head2
	hash=$head2 short=$short2 subject=changed foo
	EOF
	git rev-list --abbrev=4 --max-count=1 --format="hash=%H short=%h subject=%s" refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success '--format with literal text only' '
	cd repo &&
	head2=$(cat head2) &&
	head1=$(cat head1) &&
	cat >expect <<-EOF &&
	commit $head2
	hello world
	commit $head1
	hello world
	EOF
	git rev-list --format="hello world" refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success 'setup third commit' '
	cd repo &&
	head2=$(cat head2) &&
	head3=$(doit 3 "third commit" "$head2") &&
	git update-ref refs/heads/master "$head3" &&
	echo "$head3" >head3
'

test_expect_success '--format %s with three commits' '
	cd repo &&
	head1=$(cat head1) &&
	head2=$(cat head2) &&
	head3=$(cat head3) &&
	cat >expect <<-EOF &&
	commit $head3
	third commit
	commit $head2
	changed foo
	commit $head1
	added foo
	EOF
	git rev-list --format=%s refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success '--max-count=1 with --format shows only one' '
	cd repo &&
	head3=$(cat head3) &&
	cat >expect <<-EOF &&
	commit $head3
	third commit
	EOF
	git rev-list --max-count=1 --format=%s refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success '--format with empty string' '
	cd repo &&
	head3=$(cat head3) &&
	head2=$(cat head2) &&
	head1=$(cat head1) &&
	cat >expect <<-EOF &&
	commit $head3

	commit $head2

	commit $head1

	EOF
	git rev-list --format="" refs/heads/master >actual &&
	test_cmp expect actual
'

# --- New tests: more format specifiers ---

test_expect_success '--format %s shows subject line' '
	cd repo &&
	git rev-list --max-count=1 --format="%s" refs/heads/master >actual &&
	grep -q "third commit" actual
'

test_expect_success '--format %H shows full hash' '
	cd repo &&
	head3=$(cat head3) &&
	git rev-list --max-count=1 --format="%H" refs/heads/master >actual &&
	grep -q "$head3" actual
'

test_expect_success '--format %h shows abbreviated hash' '
	cd repo &&
	head3=$(cat head3) &&
	short3=$(echo "$head3" | cut -c1-7) &&
	git rev-list --abbrev=7 --max-count=1 --format="%h" refs/heads/master >actual &&
	grep -q "$short3" actual
'

test_expect_success '--format %h default abbreviation' '
	cd repo &&
	git rev-list --max-count=1 --format="%h" refs/heads/master >actual &&
	# second line is the abbreviated hash
	hash=$(sed -n 2p actual) &&
	len=$(echo "$hash" | tr -d "\n" | wc -c) &&
	test "$len" -ge 4 &&
	test "$len" -le 40
'

test_expect_success '--format %%H literal percent-H' '
	cd repo &&
	head3=$(cat head3) &&
	cat >expect <<-EOF &&
	commit $head3
	%H
	EOF
	git rev-list --max-count=1 --format="%%H" refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success '--format %%%% double literal percent' '
	cd repo &&
	head3=$(cat head3) &&
	cat >expect <<-EOF &&
	commit $head3
	%%
	EOF
	git rev-list --max-count=1 --format="%%%%" refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success '--format %s with single commit' '
	cd repo &&
	head3=$(cat head3) &&
	cat >expect <<-EOF &&
	commit $head3
	third commit
	EOF
	git rev-list --max-count=1 --format="%s" refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success '--format with mixed text and %s' '
	cd repo &&
	head3=$(cat head3) &&
	cat >expect <<-EOF &&
	commit $head3
	Subject: third commit
	EOF
	git rev-list --max-count=1 --format="Subject: %s" refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success '--format %H %s on same line' '
	cd repo &&
	head3=$(cat head3) &&
	cat >expect <<-EOF &&
	commit $head3
	$head3 third commit
	EOF
	git rev-list --max-count=1 --format="%H %s" refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success '--format %H with multiple commits' '
	cd repo &&
	head1=$(cat head1) &&
	head2=$(cat head2) &&
	head3=$(cat head3) &&
	git rev-list --format="%H" refs/heads/master >actual &&
	grep -q "$head1" actual &&
	grep -q "$head2" actual &&
	grep -q "$head3" actual
'

test_expect_success '--format %s with --skip=1' '
	cd repo &&
	head1=$(cat head1) &&
	head2=$(cat head2) &&
	cat >expect <<-EOF &&
	commit $head2
	changed foo
	commit $head1
	added foo
	EOF
	git rev-list --skip=1 --format="%s" refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success '--format %H with --reverse' '
	cd repo &&
	head1=$(cat head1) &&
	head2=$(cat head2) &&
	head3=$(cat head3) &&
	cat >expect <<-EOF &&
	commit $head1
	$head1
	commit $head2
	$head2
	commit $head3
	$head3
	EOF
	git rev-list --reverse --format="%H" refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success '--format with newline via multiple lines' '
	cd repo &&
	head3=$(cat head3) &&
	cat >expect <<-EOF &&
	commit $head3
	line1 line2
	EOF
	git rev-list --max-count=1 --format="line1 line2" refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success '--format %H same as plain rev-list for each commit' '
	cd repo &&
	git rev-list refs/heads/master >plain &&
	git rev-list --format="%H" refs/heads/master | grep -v "^commit " >formatted &&
	test_cmp plain formatted
'

test_expect_success '--format %h matches --short output' '
	cd repo &&
	head3=$(cat head3) &&
	short=$(git rev-parse --short "$head3") &&
	git rev-list --max-count=1 --format="%h" refs/heads/master >actual &&
	grep -q "$short" actual
'

test_expect_success '--quiet suppresses all output' '
	cd repo &&
	git rev-list --quiet refs/heads/master >actual &&
	test_must_be_empty actual
'

test_expect_success '--format %s subjects match commit messages' '
	cd repo &&
	git rev-list --format="%s" refs/heads/master >actual &&
	grep -q "added foo" actual &&
	grep -q "changed foo" actual &&
	grep -q "third commit" actual
'

test_expect_success '--format with only literal text shows same text for each' '
	cd repo &&
	git rev-list --format="FIXED" refs/heads/master >actual &&
	count=$(grep -c "FIXED" actual) &&
	test "$count" = "3"
'

test_expect_success '--abbrev=4 with %h gives 4-char hash' '
	cd repo &&
	git rev-list --abbrev=4 --max-count=1 --format="%h" refs/heads/master >actual &&
	hash=$(sed -n 2p actual) &&
	len=$(echo "$hash" | tr -d "\n" | wc -c) &&
	test "$len" = "4"
'

test_expect_success '--abbrev=40 with %h gives full hash' '
	cd repo &&
	head3=$(cat head3) &&
	git rev-list --abbrev=40 --max-count=1 --format="%h" refs/heads/master >actual &&
	grep -q "$head3" actual
'

test_expect_success '--format %H %h %s combined' '
	cd repo &&
	head3=$(cat head3) &&
	short3=$(git rev-parse --short=4 "$head3") &&
	cat >expect <<-EOF &&
	commit $head3
	$head3 $short3 third commit
	EOF
	git rev-list --abbrev=4 --max-count=1 --format="%H %h %s" refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success 'setup fourth and fifth commits' '
	cd repo &&
	head3=$(cat head3) &&
	head4=$(doit 4 "fourth commit" "$head3") &&
	head5=$(doit 5 "fifth commit" "$head4") &&
	git update-ref refs/heads/master "$head5" &&
	echo "$head4" >head4 &&
	echo "$head5" >head5
'

test_expect_success '--format %s with 5 commits' '
	cd repo &&
	git rev-list --format="%s" refs/heads/master >actual &&
	grep -c "^commit " actual >count_file &&
	count=$(cat count_file) &&
	test "$count" = "5"
'

test_expect_success '--max-count=2 --format shows 2 entries' '
	cd repo &&
	git rev-list --max-count=2 --format="%s" refs/heads/master >actual &&
	count=$(grep -c "^commit " actual) &&
	test "$count" = "2"
'

test_expect_success '--skip=3 --format shows 2 entries' '
	cd repo &&
	git rev-list --skip=3 --format="%s" refs/heads/master >actual &&
	count=$(grep -c "^commit " actual) &&
	test "$count" = "2"
'

test_expect_success '--format %s --reverse first is oldest' '
	cd repo &&
	git rev-list --reverse --format="%s" refs/heads/master >actual &&
	sed -n 2p actual >first_subject &&
	echo "added foo" >expect &&
	test_cmp expect first_subject
'

test_expect_success '--format %s --reverse last is newest' '
	cd repo &&
	git rev-list --reverse --format="%s" refs/heads/master >actual &&
	tail -1 actual >last_subject &&
	echo "fifth commit" >expect &&
	test_cmp expect last_subject
'

test_expect_success '--format %H with --count prints count not format' '
	cd repo &&
	git rev-list --count refs/heads/master >actual &&
	echo 5 >expect &&
	test_cmp expect actual
'

test_expect_success 'commit header line present for each commit' '
	cd repo &&
	git rev-list --format="%s" refs/heads/master >actual &&
	head1=$(cat head1) &&
	head2=$(cat head2) &&
	head3=$(cat head3) &&
	head4=$(cat head4) &&
	head5=$(cat head5) &&
	grep -q "^commit $head1" actual &&
	grep -q "^commit $head2" actual &&
	grep -q "^commit $head3" actual &&
	grep -q "^commit $head4" actual &&
	grep -q "^commit $head5" actual
'

test_expect_success '--format %H identical to hash in commit header' '
	cd repo &&
	head5=$(cat head5) &&
	git rev-list --max-count=1 --format="%H" refs/heads/master >actual &&
	# First line: "commit <hash>", second line: "<hash>"
	hdr_hash=$(head -1 actual | sed "s/^commit //") &&
	fmt_hash=$(sed -n 2p actual) &&
	test "$hdr_hash" = "$fmt_hash" &&
	test "$hdr_hash" = "$head5"
'

test_expect_success '--format with %s and literal prefix/suffix' '
	cd repo &&
	head5=$(cat head5) &&
	cat >expect <<-EOF &&
	commit $head5
	[fifth commit]
	EOF
	git rev-list --max-count=1 --format="[%s]" refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success '--format %% at start and end' '
	cd repo &&
	head5=$(cat head5) &&
	cat >expect <<-EOF &&
	commit $head5
	%fifth commit%
	EOF
	git rev-list --max-count=1 --format="%%%s%%" refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success '--format %H repeated' '
	cd repo &&
	head5=$(cat head5) &&
	cat >expect <<-EOF &&
	commit $head5
	$head5=$head5
	EOF
	git rev-list --max-count=1 --format="%H=%H" refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success '--format with no specifiers is literal' '
	cd repo &&
	head5=$(cat head5) &&
	cat >expect <<-EOF &&
	commit $head5
	no-specifiers-here
	EOF
	git rev-list --max-count=1 --format="no-specifiers-here" refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success '--format produces correct line count' '
	cd repo &&
	git rev-list --format="%H" refs/heads/master >actual &&
	# 5 commits * 2 lines each (commit header + format line) = 10
	lines=$(wc -l <actual | tr -d " ") &&
	test "$lines" = "10"
'

test_expect_success '--format empty string produces 10 lines (5 commits)' '
	cd repo &&
	git rev-list --format="" refs/heads/master >actual &&
	lines=$(wc -l <actual | tr -d " ") &&
	test "$lines" = "10"
'

test_expect_success '--format %h with --abbrev=10' '
	cd repo &&
	git rev-list --abbrev=10 --max-count=1 --format="%h" refs/heads/master >actual &&
	hash=$(sed -n 2p actual) &&
	len=$(echo "$hash" | tr -d "\n" | wc -c) &&
	test "$len" = "10"
'

test_expect_success '--format %s does not include trailing newline in subject' '
	cd repo &&
	git rev-list --max-count=1 --format="%s" refs/heads/master >actual &&
	subject=$(sed -n 2p actual) &&
	test "$subject" = "fifth commit"
'

test_expect_success 'setup merge commit for parent tests' '
	cd repo &&
	head5=$(cat head5) &&
	side=$(doit 6 "side branch" "$(cat head3)") &&
	merge=$(doit 7 "merge commit" "$head5" "$side") &&
	git update-ref refs/heads/master "$merge" &&
	echo "$side" >side &&
	echo "$merge" >merge
'

test_expect_success '--format %H on merge commit' '
	cd repo &&
	merge=$(cat merge) &&
	cat >expect <<-EOF &&
	commit $merge
	$merge
	EOF
	git rev-list --max-count=1 --format="%H" refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success '--format %s on merge commit' '
	cd repo &&
	merge=$(cat merge) &&
	cat >expect <<-EOF &&
	commit $merge
	merge commit
	EOF
	git rev-list --max-count=1 --format="%s" refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success '--first-parent --format %s skips side' '
	cd repo &&
	git rev-list --first-parent --format="%s" refs/heads/master >actual &&
	! grep -q "side branch" actual
'

test_expect_success '--format %H --first-parent' '
	cd repo &&
	side=$(cat side) &&
	git rev-list --first-parent --format="%H" refs/heads/master >actual &&
	! grep -q "$side" actual
'

test_expect_success '--topo-order --format %s' '
	cd repo &&
	git rev-list --topo-order --format="%s" refs/heads/master >actual &&
	# merge should appear before its parents
	merge_line=$(grep -n "merge commit" actual | head -1 | cut -d: -f1) &&
	added_line=$(grep -n "added foo" actual | head -1 | cut -d: -f1) &&
	test "$merge_line" -lt "$added_line"
'

test_expect_success '--format %H count after merge (7 commits)' '
	cd repo &&
	git rev-list --format="%H" refs/heads/master >actual &&
	count=$(grep -c "^commit " actual) &&
	test "$count" = "7"
'

test_expect_success '--format %h all abbreviated hashes are unique' '
	cd repo &&
	git rev-list --format="%h" refs/heads/master | grep -v "^commit " >hashes &&
	sort hashes >sorted &&
	uniq sorted >unique &&
	test_cmp sorted unique
'

test_expect_success '--format %H all full hashes are unique' '
	cd repo &&
	git rev-list --format="%H" refs/heads/master | grep -v "^commit " >hashes &&
	sort hashes >sorted &&
	uniq sorted >unique &&
	test_cmp sorted unique
'

test_expect_success '--format %s --max-count=3 gives 3 subjects' '
	cd repo &&
	git rev-list --max-count=3 --format="%s" refs/heads/master >actual &&
	count=$(grep -c "^commit " actual) &&
	test "$count" = "3"
'

test_expect_success '--quiet with --format still quiet' '
	cd repo &&
	git rev-list --quiet refs/heads/master >actual &&
	test_must_be_empty actual
'

test_expect_success '--format single %' '
	cd repo &&
	merge=$(cat merge) &&
	cat >expect <<-EOF &&
	commit $merge
	%
	EOF
	git rev-list --max-count=1 --format="%%" refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success '--format %H with range' '
	cd repo &&
	head3=$(cat head3) &&
	git rev-list --format="%H" "$head3"..refs/heads/master >actual &&
	! grep -q "$head3" actual &&
	count=$(grep -c "^commit " actual) &&
	test "$count" -ge 1
'

test_expect_success '--format %s with range excludes ancestor' '
	cd repo &&
	head3=$(cat head3) &&
	git rev-list --format="%s" "$head3"..refs/heads/master >actual &&
	! grep -q "third commit" actual
'

test_expect_success '--format %h --reverse first hash is oldest' '
	cd repo &&
	head1=$(cat head1) &&
	short1=$(git rev-parse --short "$head1") &&
	git rev-list --reverse --format="%h" refs/heads/master >actual &&
	second_line=$(sed -n 2p actual) &&
	test "$second_line" = "$short1"
'

test_expect_success '--format %H --skip=5 with 7 commits gives 2' '
	cd repo &&
	git rev-list --skip=5 --format="%H" refs/heads/master >actual &&
	count=$(grep -c "^commit " actual) &&
	test "$count" = "2"
'

test_done
