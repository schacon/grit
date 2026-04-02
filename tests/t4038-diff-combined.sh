#!/bin/sh
# Test diff behaviour around merge commits:
# diffing merge parents, merge vs parents, stat/numstat on merges.

test_description='diff around merge commits'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT=""
for p in /usr/bin/git /usr/local/bin/git; do
	if test -x "$p"; then
		REAL_GIT="$p"
		break
	fi
done
if test -z "$REAL_GIT"; then
	echo "SKIP: real git not found" >&2
	exit 0
fi

test_expect_success 'setup: create repo with divergent branches' '
	grit init repo &&
	cd repo &&
	git config user.email "test@test.com" &&
	git config user.name "Test" &&
	echo "base" >base.txt &&
	grit add base.txt &&
	grit commit -m "base commit" &&
	"$REAL_GIT" checkout -b side &&
	echo "side content" >side.txt &&
	grit add side.txt &&
	grit commit -m "side commit" &&
	"$REAL_GIT" checkout master &&
	echo "master content" >master.txt &&
	grit add master.txt &&
	grit commit -m "master commit"
'

test_expect_success 'create merge commit' '
	cd repo &&
	"$REAL_GIT" merge side --no-edit
'

test_expect_success 'diff merge^1 merge shows side branch changes' '
	cd repo &&
	grit diff HEAD^1 HEAD >actual &&
	grep "side.txt" actual
'

test_expect_success 'diff merge^2 merge shows master branch changes' '
	cd repo &&
	grit diff HEAD^2 HEAD >actual &&
	grep "master.txt" actual
'

test_expect_success 'diff --name-only merge^1 merge' '
	cd repo &&
	grit diff --name-only HEAD^1 HEAD >actual &&
	grep "side.txt" actual &&
	! grep "master.txt" actual
'

test_expect_success 'diff --name-only merge^2 merge' '
	cd repo &&
	grit diff --name-only HEAD^2 HEAD >actual &&
	grep "master.txt" actual &&
	! grep "side.txt" actual
'

test_expect_success 'diff --stat merge^1 merge' '
	cd repo &&
	grit diff --stat HEAD^1 HEAD >actual &&
	grep "side.txt" actual
'

test_expect_success 'diff --stat merge^2 merge' '
	cd repo &&
	grit diff --stat HEAD^2 HEAD >actual &&
	grep "master.txt" actual
'

test_expect_success 'diff --numstat merge^1 merge' '
	cd repo &&
	grit diff --numstat HEAD^1 HEAD >actual &&
	grep "side.txt" actual
'

test_expect_success 'diff --numstat merge^2 merge' '
	cd repo &&
	grit diff --numstat HEAD^2 HEAD >actual &&
	grep "master.txt" actual
'

test_expect_success 'diff --name-status merge^1 merge' '
	cd repo &&
	grit diff --name-status HEAD^1 HEAD >actual &&
	grep "A" actual &&
	grep "side.txt" actual
'

test_expect_success 'diff --name-status merge^2 merge' '
	cd repo &&
	grit diff --name-status HEAD^2 HEAD >actual &&
	grep "A" actual &&
	grep "master.txt" actual
'

# ── merge with conflicts (resolved) ──

test_expect_success 'setup conflict merge' '
	cd repo &&
	"$REAL_GIT" checkout -b conflict-a master~1 &&
	echo "version A" >conflict.txt &&
	grit add conflict.txt &&
	grit commit -m "conflict-a" &&
	"$REAL_GIT" checkout -b conflict-b master~1 &&
	echo "version B" >conflict.txt &&
	grit add conflict.txt &&
	grit commit -m "conflict-b" &&
	"$REAL_GIT" checkout conflict-a &&
	"$REAL_GIT" merge conflict-b --no-edit || true &&
	echo "resolved" >conflict.txt &&
	grit add conflict.txt &&
	grit commit -m "merge resolved"
'

test_expect_success 'diff merge^1 merge for conflict resolution' '
	cd repo &&
	grit diff HEAD^1 HEAD >actual &&
	grep "conflict.txt" actual
'

test_expect_success 'diff merge^2 merge for conflict resolution' '
	cd repo &&
	grit diff HEAD^2 HEAD >actual &&
	grep "conflict.txt" actual
'

test_expect_success 'diff --stat for conflict merge' '
	cd repo &&
	grit diff --stat HEAD^1 HEAD >actual &&
	grep "conflict.txt" actual
'

# ── merge with no changes on one side ──

test_expect_success 'setup fast-forward-like merge' '
	cd repo &&
	"$REAL_GIT" checkout -f master &&
	"$REAL_GIT" checkout -b ff-side &&
	echo "ff content" >ff.txt &&
	grit add ff.txt &&
	grit commit -m "ff side" &&
	"$REAL_GIT" checkout master &&
	"$REAL_GIT" merge --no-ff ff-side --no-edit
'

test_expect_success 'diff merge^1 merge for no-ff merge' '
	cd repo &&
	grit diff HEAD^1 HEAD >actual &&
	grep "ff.txt" actual
'

test_expect_success 'diff merge^2 merge for no-ff merge is empty' '
	cd repo &&
	grit diff HEAD^2 HEAD >actual &&
	test_must_be_empty actual
'

# ── octopus-like: multiple parents ──

test_expect_success 'setup octopus merge' '
	cd repo &&
	"$REAL_GIT" checkout -f master &&
	"$REAL_GIT" checkout -b octo-a &&
	echo "octo-a" >octo-a.txt &&
	grit add octo-a.txt &&
	grit commit -m "octo-a" &&
	"$REAL_GIT" checkout master &&
	"$REAL_GIT" checkout -b octo-b &&
	echo "octo-b" >octo-b.txt &&
	grit add octo-b.txt &&
	grit commit -m "octo-b" &&
	"$REAL_GIT" checkout -f master &&
	"$REAL_GIT" merge octo-a octo-b --no-edit
'

test_expect_success 'diff merge^1 octopus merge' '
	cd repo &&
	grit diff HEAD^1 HEAD >actual &&
	grep "octo-a.txt\|octo-b.txt" actual
'

test_expect_success 'diff --name-only merge^1 octopus shows added files' '
	cd repo &&
	grit diff --name-only HEAD^1 HEAD >actual &&
	test -s actual
'

# ── diff across merge (grandparent) ──

test_expect_success 'diff grandparent to merge' '
	cd repo &&
	GRANDPARENT=$(grit rev-list HEAD | head -3 | tail -1) &&
	grit diff $GRANDPARENT HEAD >actual &&
	test -s actual
'

test_expect_success 'diff --stat grandparent to merge' '
	cd repo &&
	GRANDPARENT=$(grit rev-list HEAD | head -3 | tail -1) &&
	grit diff --stat $GRANDPARENT HEAD >actual &&
	test -s actual
'

# ── rev-list around merges ──

test_expect_success 'rev-list counts all commits including merges' '
	cd repo &&
	grit rev-list HEAD >all &&
	COUNT=$(wc -l <all | tr -d " ") &&
	test "$COUNT" -ge 5
'

test_expect_success 'rev-list --first-parent skips side branches' '
	cd repo &&
	grit rev-list --first-parent HEAD >fp &&
	grit rev-list HEAD >all &&
	FP_COUNT=$(wc -l <fp | tr -d " ") &&
	ALL_COUNT=$(wc -l <all | tr -d " ") &&
	test "$FP_COUNT" -lt "$ALL_COUNT"
'

test_expect_success 'rev-list --max-count=1 on merge returns one commit' '
	cd repo &&
	grit rev-list --max-count=1 HEAD >actual &&
	test_line_count = 1 actual
'

test_expect_success 'rev-list exclusion with merge base' '
	cd repo &&
	grit rev-list HEAD ^HEAD~1 >actual &&
	COUNT=$(wc -l <actual | tr -d " ") &&
	test "$COUNT" -ge 1
'

test_done
