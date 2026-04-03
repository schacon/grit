#!/bin/sh
# Tests for 'grit log' format specifiers and output combinations.
# (-S pickaxe is not yet implemented; these tests comprehensively cover
# --format with all supported placeholders and their combinations.)

test_description='grit log format specifiers'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repository' '
	git init repo &&
	cd repo &&
	git config user.name "A U Thor" &&
	git config user.email "author@example.com" &&

	echo one >file &&
	git add file &&
	test_tick &&
	git commit -m "first commit" &&

	echo two >file &&
	git add file &&
	test_tick &&
	git commit -m "second commit" &&

	echo three >file &&
	git add file &&
	test_tick &&
	git commit -m "third commit"
'

# ── Individual format specifiers ─────────────────────────────────────────────

test_expect_success 'format %H shows full commit hash (40 hex chars)' '
	cd repo &&
	git log -n1 --format="%H" >actual &&
	test $(wc -c <actual) -ge 40
'

test_expect_success 'format %h shows abbreviated hash (7+ chars)' '
	cd repo &&
	git log -n1 --format="%h" >actual &&
	LEN=$(tr -d "\n" <actual | wc -c) &&
	test "$LEN" -ge 7 &&
	test "$LEN" -le 40
'

test_expect_success 'format %T shows tree hash' '
	cd repo &&
	git log -n1 --format="%T" >actual &&
	test $(tr -d "\n" <actual | wc -c) -eq 40
'

test_expect_success 'format %t shows abbreviated tree hash' '
	cd repo &&
	git log -n1 --format="%t" >actual &&
	LEN=$(tr -d "\n" <actual | wc -c) &&
	test "$LEN" -ge 7
'

test_expect_success 'format %P shows parent hash' '
	cd repo &&
	git log -n1 --format="%P" >actual &&
	test $(tr -d "\n" <actual | wc -c) -eq 40
'

test_expect_success 'format %p shows abbreviated parent hash' '
	cd repo &&
	git log -n1 --format="%p" >actual &&
	LEN=$(tr -d "\n" <actual | wc -c) &&
	test "$LEN" -ge 7
'

test_expect_success 'format %P for root commit is empty' '
	cd repo &&
	ROOT=$(git rev-list HEAD | tail -1) &&
	git log -n1 --format="%P" "$ROOT" >root_parent &&
	# Should be empty or just a newline
	test $(tr -d "\n" <root_parent | wc -c) -eq 0
'

test_expect_success 'format %an shows author name' '
	cd repo &&
	git log -n1 --format="%an" >actual &&
	echo "A U Thor" >expect &&
	test_cmp expect actual
'

test_expect_success 'format %ae shows author email' '
	cd repo &&
	git log -n1 --format="%ae" >actual &&
	echo "author@example.com" >expect &&
	test_cmp expect actual
'

test_expect_success 'format %cn shows committer name' '
	cd repo &&
	git log -n1 --format="%cn" >actual &&
	echo "C O Mitter" >expect &&
	test_cmp expect actual
'

test_expect_success 'format %ce shows committer email' '
	cd repo &&
	git log -n1 --format="%ce" >actual &&
	echo "committer@example.com" >expect &&
	test_cmp expect actual
'

test_expect_success 'format %s shows subject line' '
	cd repo &&
	git log -n1 --format="%s" >actual &&
	echo "third commit" >expect &&
	test_cmp expect actual
'

test_expect_success 'format %n inserts newline' '
	cd repo &&
	git log -n1 --format="A%nB" >actual &&
	test_line_count = 2 actual
'

# ── Combined format specifiers ───────────────────────────────────────────────

test_expect_success 'format combining hash and subject' '
	cd repo &&
	git log -n1 --format="%h %s" >actual &&
	HASH=$(git rev-parse --short HEAD) &&
	echo "$HASH third commit" >expect &&
	test_cmp expect actual
'

test_expect_success 'format with author name and email' '
	cd repo &&
	git log -n1 --format="%an <%ae>" >actual &&
	echo "A U Thor <author@example.com>" >expect &&
	test_cmp expect actual
'

test_expect_success 'format with literal text and placeholders' '
	cd repo &&
	git log -n1 --format="commit: %h by %an" >actual &&
	HASH=$(git rev-parse --short HEAD) &&
	echo "commit: $HASH by A U Thor" >expect &&
	test_cmp expect actual
'

test_expect_success 'format with separator chars' '
	cd repo &&
	git log -n1 --format="%h|%an|%ae|%s" >actual &&
	HASH=$(git rev-parse --short HEAD) &&
	echo "$HASH|A U Thor|author@example.com|third commit" >expect &&
	test_cmp expect actual
'

# ── Format across multiple commits ──────────────────────────────────────────

test_expect_success 'format %s for all commits' '
	cd repo &&
	git log --format="%s" >actual &&
	cat >expect <<-\EOF &&
	third commit
	second commit
	first commit
	EOF
	test_cmp expect actual
'

test_expect_success 'format %H for all commits gives unique hashes' '
	cd repo &&
	git log --format="%H" >actual &&
	test_line_count = 3 actual &&
	sort -u actual >unique &&
	test_line_count = 3 unique
'

test_expect_success 'format %T for all commits gives distinct trees' '
	cd repo &&
	git log --format="%T" >actual &&
	sort -u actual >unique &&
	test_line_count = 3 unique
'

test_expect_success 'format %an is same for all commits' '
	cd repo &&
	git log --format="%an" >actual &&
	sort -u actual >unique &&
	test_line_count = 1 unique
'

# ── --oneline vs --format ───────────────────────────────────────────────────

test_expect_success 'oneline format matches %h %s with decorate' '
	cd repo &&
	git log --oneline --no-decorate >oneline &&
	git log --format="%h %s" >formatted &&
	test_cmp oneline formatted
'

# ── With --author override ──────────────────────────────────────────────────

test_expect_success 'setup commit with different author' '
	cd repo &&
	echo four >file &&
	git add file &&
	test_tick &&
	git commit --author="Other Person <other@example.com>" -m "fourth commit"
'

test_expect_success 'format %an shows overridden author' '
	cd repo &&
	git log -n1 --format="%an" >actual &&
	echo "Other Person" >expect &&
	test_cmp expect actual
'

test_expect_success 'format %ae shows overridden author email' '
	cd repo &&
	git log -n1 --format="%ae" >actual &&
	echo "other@example.com" >expect &&
	test_cmp expect actual
'

test_expect_success 'format %cn still shows original committer' '
	cd repo &&
	git log -n1 --format="%cn" >actual &&
	echo "C O Mitter" >expect &&
	test_cmp expect actual
'

test_expect_success 'format %ce still shows original committer email' '
	cd repo &&
	git log -n1 --format="%ce" >actual &&
	echo "committer@example.com" >expect &&
	test_cmp expect actual
'

# ── Empty format string ─────────────────────────────────────────────────────

test_expect_success 'format with empty string produces empty lines' '
	cd repo &&
	git log --format="" >actual &&
	test_line_count = 4 actual &&
	while read line; do
		test -z "$line" || return 1
	done <actual
'

# ── tformat prefix ──────────────────────────────────────────────────────────

test_expect_success 'tformat:%s is same as format:%s for subjects' '
	cd repo &&
	git log --format="%s" >fmt &&
	git log --pretty="tformat:%s" >tfmt &&
	test_cmp fmt tfmt
'

test_done
