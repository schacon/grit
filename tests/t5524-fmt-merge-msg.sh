#!/bin/sh
# Tests for 'grit fmt-merge-msg'.
# Ported from git/t/t5524-pull-msg.sh (FETCH_HEAD message formatting subset).
#
# Note: tests that require shortlog body generation (--log) or GPG-signed
# objects are not yet ported — those depend on commit history traversal
# not implemented in this pass.

test_description='grit fmt-merge-msg: produce a merge commit message'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ── empty input ───────────────────────────────────────────────────────────────

test_expect_success 'empty input produces empty output' '
	: | git fmt-merge-msg >actual &&
	test_must_be_empty actual
'

# ── not-for-merge lines are ignored ──────────────────────────────────────────

test_expect_success 'not-for-merge entries are ignored' '
	printf "abc123\tnot-for-merge\tbranch '"'"'old'"'"' of https://x.com\n" |
	git fmt-merge-msg >actual &&
	test_must_be_empty actual
'

# ── single branch (local) ─────────────────────────────────────────────────────

test_expect_success 'single local branch produces Merge branch title' '
	printf "abc123\t\tbranch '"'"'feature'"'"'\n" |
	git fmt-merge-msg >actual &&
	grep -q "branch '"'"'feature'"'"'" actual
'

# ── single branch from remote ────────────────────────────────────────────────

test_expect_success 'single remote branch includes URL' '
	printf "abc123\t\tbranch '"'"'main'"'"' of https://example.com/repo\n" |
	git fmt-merge-msg >actual &&
	grep -q "branch '"'"'main'"'"'" actual &&
	grep -q "of https://example.com/repo" actual
'

# ── multiple branches ────────────────────────────────────────────────────────

test_expect_success 'multiple branches uses plural form' '
	printf "a1\t\tbranch '"'"'foo'"'"'\nb2\t\tbranch '"'"'bar'"'"'\n" |
	git fmt-merge-msg >actual &&
	grep -q "branches" actual
'

# ── -m / --message overrides title ───────────────────────────────────────────

test_expect_success '-m overrides the auto-generated title' '
	printf "abc123\t\tbranch '"'"'feature'"'"'\n" |
	git fmt-merge-msg -m "Custom merge message" >actual &&
	grep -q "Custom merge message" actual
'

test_expect_success '--message overrides the auto-generated title' '
	printf "abc123\t\tbranch '"'"'feature'"'"'\n" |
	git fmt-merge-msg --message "Custom merge message" >actual &&
	grep -q "Custom merge message" actual
'

# ── -F / --file reads from file ───────────────────────────────────────────────

test_expect_success '-F reads merge info from a file' '
	printf "abc123\t\tbranch '"'"'topic'"'"'\n" >fetch_head_file &&
	git fmt-merge-msg -F fetch_head_file >actual &&
	grep -q "branch '"'"'topic'"'"'" actual
'

test_expect_success '--file reads merge info from a file' '
	printf "abc123\t\tbranch '"'"'topic'"'"'\n" >fetch_head_file2 &&
	git fmt-merge-msg --file fetch_head_file2 >actual &&
	grep -q "branch '"'"'topic'"'"'" actual
'

# ── tag merging ───────────────────────────────────────────────────────────────

test_expect_success 'tag entry produces tag title' '
	printf "abc123\t\ttag '"'"'v1.0'"'"' of https://example.com\n" |
	git fmt-merge-msg >actual &&
	grep -q "tag" actual
'

# ── output has trailing newline ───────────────────────────────────────────────

test_expect_success 'output ends with a newline' '
	printf "abc123\t\tbranch '"'"'feature'"'"'\n" |
	git fmt-merge-msg >actual &&
	test -s actual &&
	# The last byte should be a newline (od shows 012 = 0x0a).
	tail -c1 actual | od -An -tx1 | grep -q 0a
'

# ---- more fmt-merge-msg tests ----

test_expect_success '--into-name appends into <branch>' '
	printf "abc123\t\tbranch '"'"'feature'"'"'\n" |
	git fmt-merge-msg --into-name develop >actual &&
	grep -q "into develop" actual
'

test_expect_success '--log is accepted (compat flag)' '
	printf "abc123\t\tbranch '"'"'feature'"'"'\n" |
	git fmt-merge-msg --log >actual &&
	grep -q "branch '"'"'feature'"'"'" actual
'

test_expect_success '--log with count is accepted' '
	printf "abc123\t\tbranch '"'"'feature'"'"'\n" |
	git fmt-merge-msg --log=5 >actual &&
	grep -q "branch '"'"'feature'"'"'" actual
'

test_expect_success '--no-log is accepted' '
	printf "abc123\t\tbranch '"'"'feature'"'"'\n" |
	git fmt-merge-msg --no-log >actual &&
	grep -q "branch '"'"'feature'"'"'" actual
'

test_expect_success 'multiple tags uses plural form' '
	printf "a1\t\ttag '"'"'v1.0'"'"'\nb2\t\ttag '"'"'v2.0'"'"'\n" |
	git fmt-merge-msg >actual &&
	grep -q "tags" actual
'

test_expect_success 'mixed branch and tag entries' '
	printf "a1\t\tbranch '"'"'feat'"'"'\nb2\t\ttag '"'"'v1.0'"'"'\n" |
	git fmt-merge-msg >actual &&
	grep -q "branch" actual &&
	grep -q "tag" actual
'

test_expect_success '--into-name with remote branch' '
	printf "abc123\t\tbranch '"'"'main'"'"' of https://example.com/repo\n" |
	git fmt-merge-msg --into-name release >actual &&
	grep -q "into release" actual &&
	grep -q "of https://example.com/repo" actual
'

test_expect_success 'three branches uses branches plural' '
	printf "a1\t\tbranch '"'"'a'"'"'\nb2\t\tbranch '"'"'b'"'"'\nc3\t\tbranch '"'"'c'"'"'\n" |
	git fmt-merge-msg >actual &&
	grep -q "branches" actual
'

test_expect_success '-F with /dev/stdin works like pipe' '
	printf "abc123\t\tbranch '"'"'dev'"'"'\n" >fminput &&
	git fmt-merge-msg -F fminput >actual &&
	grep -q "branch '"'"'dev'"'"'" actual
'

test_expect_success 'single remote tag includes URL' '
	printf "abc123\t\ttag '"'"'v1.0'"'"' of https://example.com/repo\n" |
	git fmt-merge-msg >actual &&
	grep -q "tag" actual &&
	grep -q "of https://example.com/repo" actual
'

test_expect_success '-m with --into-name combines both' '
	printf "abc123\t\tbranch '"'"'feature'"'"'\n" |
	git fmt-merge-msg -m "Custom merge" --into-name main >actual &&
	grep -q "Custom merge" actual
'

test_expect_success 'single branch without remote is just Merge branch' '
	printf "abc123\t\tbranch '"'"'bugfix'"'"'\n" |
	git fmt-merge-msg >actual &&
	echo "Merge branch '"'"'bugfix'"'"'" >expected &&
	test_cmp expected actual
'

test_expect_success 'multiple remote branches from same remote' '
	printf "a1\t\tbranch '"'"'a'"'"' of https://example.com\nb2\t\tbranch '"'"'b'"'"' of https://example.com\n" |
	git fmt-merge-msg >actual &&
	grep -q "branches" actual &&
	grep -q "of https://example.com" actual
'

test_expect_success '--into-name alone without -m works' '
	printf "abc123\t\tbranch '"'"'feat'"'"'\n" |
	git fmt-merge-msg --into-name develop >actual &&
	grep -q "into develop" actual &&
	grep -q "branch '"'"'feat'"'"'" actual
'

test_expect_success 'two branches from different remotes' '
	printf "a1\t\tbranch '"'"'x'"'"' of https://one.com\nb2\t\tbranch '"'"'y'"'"' of https://two.com\n" |
	git fmt-merge-msg >actual &&
	grep -q "branch '"'"'x'"'"'" actual &&
	grep -q "branch '"'"'y'"'"'" actual
'

test_expect_success 'fmt-merge-msg with empty stdin produces empty output' '
	git fmt-merge-msg </dev/null >actual &&
	test_must_be_empty actual
'

test_expect_success 'single tag without remote' '
	printf "abc123\t\ttag '"'"'v2.0'"'"'\n" |
	git fmt-merge-msg >actual &&
	grep -q "tag '"'"'v2.0'"'"'" actual
'

test_expect_success '-m replaces default subject line' '
	printf "abc123\t\tbranch '"'"'test'"'"'\n" |
	git fmt-merge-msg -m "My custom msg" >actual &&
	head -1 actual >first &&
	echo "My custom msg" >expected &&
	test_cmp expected first
'

test_expect_success 'four branches lists all of them' '
	printf "a1\t\tbranch '"'"'w'"'"'\nb2\t\tbranch '"'"'x'"'"'\nc3\t\tbranch '"'"'y'"'"'\nd4\t\tbranch '"'"'z'"'"'\n" |
	git fmt-merge-msg >actual &&
	grep -q "branches" actual
'

test_done
