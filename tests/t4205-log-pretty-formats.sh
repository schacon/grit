#!/bin/sh
#
# Tests for grit log --format / --pretty with format placeholders.
# Ported / written for the grit (Git-in-Rust) project.

test_description='grit log --format pretty-print placeholders'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ---------------------------------------------------------------------------
# Setup: create a small repo with multiple commits so we can test parent hashes,
# body text, multi-commit output, etc.
# ---------------------------------------------------------------------------

test_expect_success 'setup: create test repository' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo "hello" >file &&
	git add file &&
	GIT_AUTHOR_DATE="1700000000 +0000" GIT_COMMITTER_DATE="1700000000 +0000" \
		git commit -m "initial commit" &&
	echo "world" >>file &&
	git add file &&
	GIT_AUTHOR_DATE="1700000100 +0000" GIT_COMMITTER_DATE="1700000100 +0000" \
		git commit -m "second commit

This is the body of the second commit.
It has multiple lines."
'

# ===========================================================================
# %H — full commit hash
# ===========================================================================

test_expect_success '%H shows full commit hash (HEAD)' '
	cd repo &&
	hash=$(git rev-parse HEAD) &&
	out=$(git log --format="%H" -n 1) &&
	test "$out" = "$hash"
'

test_expect_success '%H shows full hash for earlier commit (--skip)' '
	cd repo &&
	hash=$(git rev-parse HEAD~1) &&
	out=$(git log --format="%H" --skip=1 -n 1) &&
	test "$out" = "$hash"
'

test_expect_success '%H is always exactly 40 hex characters' '
	cd repo &&
	out=$(git log --format="%H" -n 1) &&
	len=$(printf "%s" "$out" | wc -c | tr -d " ") &&
	test "$len" = "40" &&
	echo "$out" | grep -qE "^[0-9a-f]{40}$"
'

# ===========================================================================
# %h — abbreviated commit hash
# ===========================================================================

test_expect_success '%h shows abbreviated commit hash' '
	cd repo &&
	abbrev=$(git rev-parse --short HEAD) &&
	out=$(git log --format="%h" -n 1) &&
	test "$out" = "$abbrev"
'

test_expect_success '%h is 7 characters by default' '
	cd repo &&
	out=$(git log --format="%h" -n 1) &&
	len=$(printf "%s" "$out" | wc -c | tr -d " ") &&
	test "$len" = "7"
'

test_expect_success '%h is a prefix of %H' '
	cd repo &&
	full=$(git log --format="%H" -n 1) &&
	short=$(git log --format="%h" -n 1) &&
	case "$full" in
	"$short"*) : ok ;;
	*) echo "FAIL: $short is not prefix of $full"; exit 1 ;;
	esac
'

# ===========================================================================
# %T — full tree hash
# ===========================================================================

test_expect_success '%T shows full tree hash' '
	cd repo &&
	tree=$(git rev-parse HEAD^{tree}) &&
	out=$(git log --format="%T" -n 1) &&
	test "$out" = "$tree"
'

test_expect_success '%T is always exactly 40 hex characters' '
	cd repo &&
	out=$(git log --format="%T" -n 1) &&
	len=$(printf "%s" "$out" | wc -c | tr -d " ") &&
	test "$len" = "40" &&
	echo "$out" | grep -qE "^[0-9a-f]{40}$"
'

# ===========================================================================
# %t — abbreviated tree hash
# ===========================================================================

test_expect_success '%t shows abbreviated tree hash' '
	cd repo &&
	tree_abbrev=$(git rev-parse --short HEAD^{tree}) &&
	out=$(git log --format="%t" -n 1) &&
	test "$out" = "$tree_abbrev"
'

test_expect_success '%t is a prefix of %T' '
	cd repo &&
	full=$(git log --format="%T" -n 1) &&
	short=$(git log --format="%t" -n 1) &&
	case "$full" in
	"$short"*) : ok ;;
	*) echo "FAIL: $short is not prefix of $full"; exit 1 ;;
	esac
'

# ===========================================================================
# %P — full parent hash
# ===========================================================================

test_expect_success '%P shows full parent hash' '
	cd repo &&
	parent=$(git rev-parse HEAD~1) &&
	out=$(git log --format="%P" -n 1) &&
	test "$out" = "$parent"
'

test_expect_success '%P is empty for root commit' '
	cd repo &&
	out=$(git log --format="%P" --skip=1 -n 1) &&
	test -z "$out"
'

# ===========================================================================
# %p — abbreviated parent hash
# ===========================================================================

test_expect_success '%p shows abbreviated parent hash' '
	cd repo &&
	parent_abbrev=$(git rev-parse --short HEAD~1) &&
	out=$(git log --format="%p" -n 1) &&
	test "$out" = "$parent_abbrev"
'

test_expect_success '%p is empty for root commit' '
	cd repo &&
	out=$(git log --format="%p" --skip=1 -n 1) &&
	test -z "$out"
'

test_expect_success '%p is a prefix of %P' '
	cd repo &&
	full=$(git log --format="%P" -n 1) &&
	short=$(git log --format="%p" -n 1) &&
	case "$full" in
	"$short"*) : ok ;;
	*) echo "FAIL: $short is not prefix of $full"; exit 1 ;;
	esac
'

# ===========================================================================
# %an — author name
# ===========================================================================

test_expect_success '%an shows author name' '
	cd repo &&
	out=$(git log --format="%an" -n 1) &&
	test "$out" = "Test User"
'

# ===========================================================================
# %ae — author email
# ===========================================================================

test_expect_success '%ae shows author email' '
	cd repo &&
	out=$(git log --format="%ae" -n 1) &&
	test "$out" = "test@example.com"
'

# ===========================================================================
# %cn — committer name
# ===========================================================================

test_expect_success '%cn shows committer name' '
	cd repo &&
	out=$(git log --format="%cn" -n 1) &&
	test "$out" = "Test User"
'

# ===========================================================================
# %ce — committer email
# ===========================================================================

test_expect_success '%ce shows committer email' '
	cd repo &&
	out=$(git log --format="%ce" -n 1) &&
	test "$out" = "test@example.com"
'

# ===========================================================================
# %s — subject line
# ===========================================================================

test_expect_success '%s shows subject of HEAD' '
	cd repo &&
	out=$(git log --format="%s" -n 1) &&
	test "$out" = "second commit"
'

test_expect_success '%s shows subject of root commit' '
	cd repo &&
	out=$(git log --format="%s" --skip=1 -n 1) &&
	test "$out" = "initial commit"
'

# ===========================================================================
# %b — body
# ===========================================================================

test_expect_success '%b shows commit body' '
	cd repo &&
	git log --format="%b" -n 1 >actual_body &&
	printf "This is the body of the second commit.\nIt has multiple lines.\n" >expected_body &&
	test_cmp expected_body actual_body
'

test_expect_success '%b is empty for commit without body' '
	cd repo &&
	out=$(git log --format="%b" --skip=1 -n 1) &&
	test -z "$out"
'

# ===========================================================================
# %n — newline
# ===========================================================================

test_expect_success '%n produces a newline' '
	cd repo &&
	git log --format="a%nb" -n 1 >actual_nl &&
	printf "a\nb\n" >expected_nl &&
	test_cmp expected_nl actual_nl
'

test_expect_success '%n%n produces two consecutive newlines' '
	cd repo &&
	git log --format="a%n%nb" -n 1 >actual_nn &&
	printf "a\n\nb\n" >expected_nn &&
	test_cmp expected_nn actual_nn
'

# ===========================================================================
# %% — literal percent
# ===========================================================================

test_expect_success '%% produces a literal percent sign' '
	cd repo &&
	out=$(git log --format="%%" -n 1) &&
	test "$out" = "%"
'

test_expect_success 'multiple %% produce multiple percent signs' '
	cd repo &&
	out=$(git log --format="%% %% %%" -n 1) &&
	test "$out" = "% % %"
'

test_expect_success 'trailing %% at end of format' '
	cd repo &&
	out=$(git log --format="end%%" -n 1) &&
	test "$out" = "end%"
'

test_expect_success 'leading %% at start of format' '
	cd repo &&
	out=$(git log --format="%%start" -n 1) &&
	test "$out" = "%start"
'

test_expect_success '100%% produces 100 percent' '
	cd repo &&
	out=$(git log --format="100%%" -n 1) &&
	test "$out" = "100%"
'

# ===========================================================================
# Combined format placeholders
# ===========================================================================

test_expect_success '%H %h together in one format string' '
	cd repo &&
	hash=$(git rev-parse HEAD) &&
	abbrev=$(git rev-parse --short HEAD) &&
	out=$(git log --format="%H %h" -n 1) &&
	test "$out" = "$hash $abbrev"
'

test_expect_success '%an <%ae> produces author identity' '
	cd repo &&
	out=$(git log --format="%an <%ae>" -n 1) &&
	test "$out" = "Test User <test@example.com>"
'

test_expect_success '%cn <%ce> produces committer identity' '
	cd repo &&
	out=$(git log --format="%cn <%ce>" -n 1) &&
	test "$out" = "Test User <test@example.com>"
'

test_expect_success '%s (%h) subject with abbreviated hash' '
	cd repo &&
	abbrev=$(git rev-parse --short HEAD) &&
	out=$(git log --format="%s (%h)" -n 1) &&
	test "$out" = "second commit ($abbrev)"
'

test_expect_success 'multi-line format with %n' '
	cd repo &&
	hash=$(git rev-parse HEAD) &&
	git log --format="commit %H%nAuthor: %an <%ae>%n%n    %s" -n 1 >actual_multi &&
	printf "commit %s\nAuthor: Test User <test@example.com>\n\n    second commit\n" "$hash" >expected_multi &&
	test_cmp expected_multi actual_multi
'

test_expect_success 'static text around placeholders: [%h] %s' '
	cd repo &&
	out=$(git log --format="[%h] %s" -n 1) &&
	abbrev=$(git rev-parse --short HEAD) &&
	test "$out" = "[$abbrev] second commit"
'

test_expect_success 'format with colons and dashes' '
	cd repo &&
	out=$(git log --format="hash: %H -- by %an" -n 1) &&
	hash=$(git rev-parse HEAD) &&
	test "$out" = "hash: $hash -- by Test User"
'

test_expect_success 'format with parentheses' '
	cd repo &&
	out=$(git log --format="(%an) <%ae>" -n 1) &&
	test "$out" = "(Test User) <test@example.com>"
'

test_expect_success 'format with tab characters' '
	cd repo &&
	out=$(git log --format="%h	%s" -n 1) &&
	abbrev=$(git rev-parse --short HEAD) &&
	expected=$(printf "%s\tsecond commit" "$abbrev") &&
	test "$out" = "$expected"
'

# ===========================================================================
# format: and tformat: prefixes
# ===========================================================================

test_expect_success 'format: prefix works same as bare format' '
	cd repo &&
	out_bare=$(git log --format="%H" -n 1) &&
	out_prefix=$(git log --format="format:%H" -n 1) &&
	test "$out_bare" = "$out_prefix"
'

test_expect_success 'tformat: produces same output as format for single commit' '
	cd repo &&
	out_format=$(git log --format="format:%H" -n 1) &&
	out_tformat=$(git log --format="tformat:%H" -n 1) &&
	test "$out_format" = "$out_tformat"
'

test_expect_success 'tformat: works with multiple commits' '
	cd repo &&
	git log --format="tformat:%h %s" -n 2 >actual_tformat &&
	git log --format="%h %s" -n 2 >expected_tformat &&
	test_cmp expected_tformat actual_tformat
'

test_expect_success 'tformat:%s over all commits' '
	cd repo &&
	git log --format="tformat:%s" >actual_tformat_all &&
	git log --format="%s" >expected_tformat_all &&
	test_cmp expected_tformat_all actual_tformat_all
'

# ===========================================================================
# --pretty=format: syntax
# ===========================================================================

test_expect_success '--pretty=format: works like --format' '
	cd repo &&
	out_format=$(git log --format="%h %s" -n 1) &&
	out_pretty=$(git log --pretty="format:%h %s" -n 1) &&
	test "$out_format" = "$out_pretty"
'

test_expect_success '--pretty=format:%H matches --format=%H over all commits' '
	cd repo &&
	git log --pretty="format:%H" >actual_pretty_all &&
	git log --format="%H" >expected_pretty_all &&
	test_cmp expected_pretty_all actual_pretty_all
'

# ===========================================================================
# -n limiting
# ===========================================================================

test_expect_success '-n 1 limits output to one commit' '
	cd repo &&
	git log --format="%H" -n 1 >actual_n1 &&
	test_line_count = 1 actual_n1
'

test_expect_success '-n 2 shows two commits' '
	cd repo &&
	git log --format="%H" -n 2 >actual_n2 &&
	test_line_count = 2 actual_n2
'

test_expect_success '-n 100 with only 2 commits shows 2 lines' '
	cd repo &&
	git log --format="%H" -n 100 >actual_n100 &&
	test_line_count = 2 actual_n100
'

# ===========================================================================
# --skip
# ===========================================================================

test_expect_success '--skip=1 skips first commit' '
	cd repo &&
	hash_first=$(git rev-parse HEAD~1) &&
	out=$(git log --format="%H" --skip=1 -n 1) &&
	test "$out" = "$hash_first"
'

# ===========================================================================
# Multi-commit output ordering
# ===========================================================================

test_expect_success 'log output is in reverse chronological order' '
	cd repo &&
	git log --format="%s" -n 2 >actual_order &&
	printf "second commit\ninitial commit\n" >expected_order &&
	test_cmp expected_order actual_order
'

test_expect_success 'format with %h %s over multiple commits matches' '
	cd repo &&
	h1=$(git rev-parse --short HEAD) &&
	h2=$(git rev-parse --short HEAD~1) &&
	git log --format="%h %s" -n 2 >actual_hs &&
	printf "%s second commit\n%s initial commit\n" "$h1" "$h2" >expected_hs &&
	test_cmp expected_hs actual_hs
'

# ===========================================================================
# %H over all commits matches rev-list
# ===========================================================================

test_expect_success '%H over all commits matches rev-list' '
	cd repo &&
	git log --format="%H" >actual_all_H &&
	git rev-list HEAD >expected_all_H &&
	test_cmp expected_all_H actual_all_H
'

test_expect_success '%h over all commits has correct count' '
	cd repo &&
	git log --format="%h" >actual_all_h &&
	count=$(git rev-list --count HEAD) &&
	test_line_count = "$count" actual_all_h
'

# ===========================================================================
# Different author/committer scenarios
# ===========================================================================

test_expect_success 'setup: commit with different author and committer' '
	cd repo &&
	echo "extra" >>file &&
	git add file &&
	GIT_AUTHOR_NAME="Alice Author" \
	GIT_AUTHOR_EMAIL="alice@example.com" \
	GIT_AUTHOR_DATE="1700000200 +0000" \
	GIT_COMMITTER_NAME="Bob Committer" \
	GIT_COMMITTER_EMAIL="bob@example.com" \
	GIT_COMMITTER_DATE="1700000300 +0000" \
	git commit -m "third commit by alice"
'

test_expect_success '%an shows author name different from committer' '
	cd repo &&
	out=$(git log --format="%an" -n 1) &&
	test "$out" = "Alice Author"
'

test_expect_success '%ae shows author email different from committer' '
	cd repo &&
	out=$(git log --format="%ae" -n 1) &&
	test "$out" = "alice@example.com"
'

test_expect_success '%cn shows committer name different from author' '
	cd repo &&
	out=$(git log --format="%cn" -n 1) &&
	test "$out" = "Bob Committer"
'

test_expect_success '%ce shows committer email different from author' '
	cd repo &&
	out=$(git log --format="%ce" -n 1) &&
	test "$out" = "bob@example.com"
'

test_expect_success '%an <%ae> vs %cn <%ce> differ when author != committer' '
	cd repo &&
	author=$(git log --format="%an <%ae>" -n 1) &&
	committer=$(git log --format="%cn <%ce>" -n 1) &&
	test "$author" = "Alice Author <alice@example.com>" &&
	test "$committer" = "Bob Committer <bob@example.com>"
'

# ===========================================================================
# Tree hash changes between commits
# ===========================================================================

test_expect_success '%T differs between commits with different trees' '
	cd repo &&
	tree1=$(git log --format="%T" -n 1) &&
	tree2=$(git log --format="%T" --skip=1 -n 1) &&
	test "$tree1" != "$tree2"
'

test_expect_success '%t differs between commits with different trees' '
	cd repo &&
	tree1=$(git log --format="%t" -n 1) &&
	tree2=$(git log --format="%t" --skip=1 -n 1) &&
	test "$tree1" != "$tree2"
'

# ===========================================================================
# Repeated placeholder in single format
# ===========================================================================

test_expect_success 'repeated %h in one format string' '
	cd repo &&
	abbrev=$(git rev-parse --short HEAD) &&
	out=$(git log --format="%h %h %h" -n 1) &&
	test "$out" = "$abbrev $abbrev $abbrev"
'

test_expect_success 'repeated %an in one format string' '
	cd repo &&
	out=$(git log --format="%an and %an" -n 1) &&
	test "$out" = "Alice Author and Alice Author"
'

# ===========================================================================
# %% and %n combinations
# ===========================================================================

test_expect_success '%% and %n together' '
	cd repo &&
	git log --format="100%%%n200%%" -n 1 >actual_pn &&
	printf "100%%\n200%%\n" >expected_pn &&
	test_cmp expected_pn actual_pn
'

# ===========================================================================
# Subject with special characters
# ===========================================================================

test_expect_success 'setup: commit with special chars in subject' '
	cd repo &&
	echo "special" >special_file &&
	git add special_file &&
	GIT_AUTHOR_DATE="1700000400 +0000" GIT_COMMITTER_DATE="1700000400 +0000" \
		git commit -m "fix: handle <angle> & \"quotes\" in paths"
'

test_expect_success '%s preserves special characters in subject' '
	cd repo &&
	out=$(git log --format="%s" -n 1) &&
	test "$out" = "fix: handle <angle> & \"quotes\" in paths"
'

# ===========================================================================
# %b empty for bodyless commit
# ===========================================================================

test_expect_success '%b is empty for most recent bodyless commit' '
	cd repo &&
	out=$(git log --format="%b" -n 1) &&
	test -z "$out"
'

# ===========================================================================
# %b body from earlier commit (second commit had a body)
# ===========================================================================

test_expect_success '%b shows body of second commit via --skip' '
	cd repo &&
	git log --format="%b" --skip=2 -n 1 >actual_body2 &&
	printf "This is the body of the second commit.\nIt has multiple lines.\n" >expected_body2 &&
	test_cmp expected_body2 actual_body2
'

# ===========================================================================
# Cross-reference: %h and %t have same length
# ===========================================================================

test_expect_success '%h and %t have same abbreviation length' '
	cd repo &&
	h_len=$(git log --format="%h" -n 1 | wc -c | tr -d " ") &&
	t_len=$(git log --format="%t" -n 1 | wc -c | tr -d " ") &&
	test "$h_len" = "$t_len"
'

# ===========================================================================
# More edge cases
# ===========================================================================

test_expect_success '%H with --skip=0 is same as without skip' '
	cd repo &&
	out1=$(git log --format="%H" -n 1) &&
	out2=$(git log --format="%H" --skip=0 -n 1) &&
	test "$out1" = "$out2"
'

test_expect_success 'all commits reachable via log match rev-list count' '
	cd repo &&
	log_count=$(git log --format="%H" | wc -l | tr -d " ") &&
	rev_count=$(git rev-list --count HEAD) &&
	test "$log_count" = "$rev_count"
'

test_expect_success '%T matches rev-parse HEAD^{tree} for all commits' '
	cd repo &&
	git log --format="%H %T" >ht_pairs &&
	while read hash tree; do
		expected=$(git rev-parse "$hash^{tree}") &&
		test "$tree" = "$expected" || exit 1
	done <ht_pairs
'

# ===========================================================================
# Multiple format runs produce consistent results
# ===========================================================================

test_expect_success 'running log twice produces identical output' '
	cd repo &&
	git log --format="%H %h %T %t %an %ae %s" >run1 &&
	git log --format="%H %h %T %t %an %ae %s" >run2 &&
	test_cmp run1 run2
'

# ===========================================================================
# Format with no placeholders (just literal text)
# ===========================================================================

test_expect_success 'format with only literal text (no placeholders)' '
	cd repo &&
	out=$(git log --format="hello world" -n 1) &&
	test "$out" = "hello world"
'

test_expect_success 'literal text repeated for each commit' '
	cd repo &&
	git log --format="x" -n 2 >actual_literal &&
	printf "x\nx\n" >expected_literal &&
	test_cmp expected_literal actual_literal
'

# ===========================================================================
# Complex combinations
# ===========================================================================

test_expect_success 'format: hash=%H tree=%T parent=%P' '
	cd repo &&
	hash=$(git rev-parse HEAD) &&
	tree=$(git rev-parse HEAD^{tree}) &&
	parent=$(git rev-parse HEAD~1) &&
	out=$(git log --format="hash=%H tree=%T parent=%P" -n 1) &&
	test "$out" = "hash=$hash tree=$tree parent=$parent"
'

test_expect_success 'format: author=%an/%ae committer=%cn/%ce' '
	cd repo &&
	out=$(git log --format="author=%an/%ae committer=%cn/%ce" -n 1) &&
	test "$out" = "author=Test User/test@example.com committer=Test User/test@example.com"
'

test_expect_success 'format with all basic placeholders combined' '
	cd repo &&
	hash=$(git rev-parse HEAD) &&
	abbrev=$(git rev-parse --short HEAD) &&
	tree=$(git rev-parse HEAD^{tree}) &&
	tree_s=$(git rev-parse --short HEAD^{tree}) &&
	parent=$(git rev-parse HEAD~1) &&
	parent_s=$(git rev-parse --short HEAD~1) &&
	expected="$hash $abbrev $tree $tree_s $parent $parent_s Test User test@example.com Test User test@example.com" &&
	out=$(git log --format="%H %h %T %t %P %p %an %ae %cn %ce" -n 1) &&
	test "$out" = "$expected"
'

# ===========================================================================
# Setup another commit for more variety
# ===========================================================================

test_expect_success 'setup: commit with unicode in name' '
	cd repo &&
	echo "unicode" >unicode_file &&
	git add unicode_file &&
	GIT_AUTHOR_NAME="José García" \
	GIT_AUTHOR_EMAIL="jose@example.com" \
	GIT_AUTHOR_DATE="1700000500 +0000" \
	GIT_COMMITTER_DATE="1700000500 +0000" \
	git commit -m "add unicode file"
'

test_expect_success '%an handles unicode author name' '
	cd repo &&
	out=$(git log --format="%an" -n 1) &&
	test "$out" = "José García"
'

test_expect_success '%ae shows email for unicode-named author' '
	cd repo &&
	out=$(git log --format="%ae" -n 1) &&
	test "$out" = "jose@example.com"
'

# ===========================================================================
# Verify %P chain through multiple commits
# ===========================================================================

test_expect_success '%P chains correctly through history' '
	cd repo &&
	h1=$(git log --format="%H" -n 1) &&
	p1=$(git log --format="%P" -n 1) &&
	h2=$(git log --format="%H" --skip=1 -n 1) &&
	test "$p1" = "$h2"
'

# ===========================================================================
# Verify --skip with format across history
# ===========================================================================

test_expect_success '--skip=2 -n 1 gets correct commit' '
	cd repo &&
	s=$(git log --format="%s" --skip=2 -n 1) &&
	test "$s" = "third commit by alice"
'

test_expect_success '--skip=3 -n 1 gets second commit' '
	cd repo &&
	s=$(git log --format="%s" --skip=3 -n 1) &&
	test "$s" = "second commit"
'

test_expect_success '--skip=4 -n 1 gets initial commit' '
	cd repo &&
	s=$(git log --format="%s" --skip=4 -n 1) &&
	test "$s" = "initial commit"
'

# ===========================================================================
# %n at different positions
# ===========================================================================

test_expect_success '%n at beginning of format' '
	cd repo &&
	git log --format="%n%s" -n 1 >actual_nstart &&
	printf "\nadd unicode file\n" >expected_nstart &&
	test_cmp expected_nstart actual_nstart
'

test_expect_success '%n at end of format' '
	cd repo &&
	git log --format="%s%n" -n 1 >actual_nend &&
	printf "add unicode file\n\n" >expected_nend &&
	test_cmp expected_nend actual_nend
'

test_expect_success 'three %n in a row' '
	cd repo &&
	git log --format="a%n%n%nb" -n 1 >actual_3n &&
	printf "a\n\n\nb\n" >expected_3n &&
	test_cmp expected_3n actual_3n
'

# ===========================================================================
# Empty subject/body edge case
# ===========================================================================

test_expect_success 'setup: commit with empty body (single-line message)' '
	cd repo &&
	echo "oneline" >oneline_file &&
	git add oneline_file &&
	GIT_AUTHOR_DATE="1700000600 +0000" GIT_COMMITTER_DATE="1700000600 +0000" \
		git commit -m "single line only"
'

test_expect_success '%s shows single-line message' '
	cd repo &&
	out=$(git log --format="%s" -n 1) &&
	test "$out" = "single line only"
'

test_expect_success '%b is empty for single-line commit message' '
	cd repo &&
	out=$(git log --format="%b" -n 1) &&
	test -z "$out"
'

test_done
