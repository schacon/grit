#!/bin/sh
# Tests for grit name-rev.
#
# Commit graph created in setup:
#
#   A <-- B <-- C   (refs/heads/main)
#   ^
#   refs/tags/v1.0 (lightweight)
#
# A is tagged by v1.0.  Tags beat branch names, so:
#   A → tags/v1.0
#   B → main~1
#   C → main

test_description='grit name-rev basic behaviours'

. ./test-lib.sh

EMPTY_TREE=""

# ------------------------------------------------------------------
# Setup: a small linear commit graph with one tag.
# ------------------------------------------------------------------

test_expect_success 'setup repo' '
	git init repo &&
	cd repo &&
	EMPTY_TREE=$(printf "" | git hash-object -w -t tree --stdin) &&
	printf "%s" "$EMPTY_TREE" >.empty_tree &&

	GIT_COMMITTER_DATE="1000000 +0000" GIT_AUTHOR_DATE="1000000 +0000" \
		A=$(git commit-tree "$EMPTY_TREE" -m "commit A") &&
	GIT_COMMITTER_DATE="1000001 +0000" GIT_AUTHOR_DATE="1000001 +0000" \
		B=$(git commit-tree "$EMPTY_TREE" -p "$A" -m "commit B") &&
	GIT_COMMITTER_DATE="1000002 +0000" GIT_AUTHOR_DATE="1000002 +0000" \
		C=$(git commit-tree "$EMPTY_TREE" -p "$B" -m "commit C") &&

	git update-ref refs/heads/main "$C" &&
	git update-ref refs/tags/v1.0 "$A" &&

	printf "%s\n" "$A" >.oid_A &&
	printf "%s\n" "$B" >.oid_B &&
	printf "%s\n" "$C" >.oid_C
'

# ------------------------------------------------------------------
# 1. Name the commit at the branch tip.
# ------------------------------------------------------------------
test_expect_success 'name commit at branch tip' '
	cd repo &&
	C=$(cat .oid_C) &&
	printf "%s main\n" "$C" >expect &&
	git name-rev "$C" >actual &&
	test_cmp expect actual
'

# ------------------------------------------------------------------
# 2. Commit B is one first-parent hop from main.
# ------------------------------------------------------------------
test_expect_success 'commit one hop from branch tip is named main~1' '
	cd repo &&
	B=$(cat .oid_B) &&
	printf "%s main~1\n" "$B" >expect &&
	git name-rev "$B" >actual &&
	test_cmp expect actual
'

# ------------------------------------------------------------------
# 3. Tags beat branch names — A is named by its tag.
# ------------------------------------------------------------------
test_expect_success 'tag beats branch name for tagged commit' '
	cd repo &&
	A=$(cat .oid_A) &&
	printf "%s tags/v1.0\n" "$A" >expect &&
	git name-rev "$A" >actual &&
	test_cmp expect actual
'

# ------------------------------------------------------------------
# 4. --name-only suppresses the leading OID.
# ------------------------------------------------------------------
test_expect_success '--name-only prints only the name' '
	cd repo &&
	C=$(cat .oid_C) &&
	printf "main\n" >expect &&
	git name-rev --name-only "$C" >actual &&
	test_cmp expect actual
'

# ------------------------------------------------------------------
# 5. --tags restricts naming to tag refs; tag name is shown with
#    its full sub-namespace (tags/).
# ------------------------------------------------------------------
test_expect_success '--tags uses only tag refs' '
	cd repo &&
	A=$(cat .oid_A) &&
	printf "%s tags/v1.0\n" "$A" >expect &&
	git name-rev --tags "$A" >actual &&
	test_cmp expect actual
'

# ------------------------------------------------------------------
# 6. --tags --name-only shortens the tag name (strips "tags/").
# ------------------------------------------------------------------
test_expect_success '--tags --name-only shortens to bare tag name' '
	cd repo &&
	A=$(cat .oid_A) &&
	printf "v1.0\n" >expect &&
	git name-rev --tags --name-only "$A" >actual &&
	test_cmp expect actual
'

# ------------------------------------------------------------------
# 7. Commit not reachable from any ref yields "undefined" by default.
# ------------------------------------------------------------------
test_expect_success 'unreachable commit yields undefined' '
	cd repo &&
	EMPTY_TREE=$(cat .empty_tree) &&
	ORPHAN=$(git commit-tree "$EMPTY_TREE" -m "orphan") &&
	printf "%s undefined\n" "$ORPHAN" >expect &&
	git name-rev "$ORPHAN" >actual &&
	test_cmp expect actual
'

# ------------------------------------------------------------------
# 8. --no-undefined exits non-zero when no name is found.
# ------------------------------------------------------------------
test_expect_success '--no-undefined fails on unreachable commit' '
	cd repo &&
	EMPTY_TREE=$(cat .empty_tree) &&
	ORPHAN=$(git commit-tree "$EMPTY_TREE" -m "orphan2") &&
	test_must_fail git name-rev --no-undefined "$ORPHAN"
'

# ------------------------------------------------------------------
# 9. --always falls back to abbreviated hash when no name found.
# ------------------------------------------------------------------
test_expect_success '--always shows abbreviated hash as fallback' '
	cd repo &&
	EMPTY_TREE=$(cat .empty_tree) &&
	ORPHAN=$(git commit-tree "$EMPTY_TREE" -m "orphan3") &&
	SHORT=$(printf "%.7s" "$ORPHAN") &&
	printf "%s %s\n" "$ORPHAN" "$SHORT" >expect &&
	git name-rev --no-undefined --always "$ORPHAN" >actual &&
	test_cmp expect actual
'

# ------------------------------------------------------------------
# 10. --all names every reachable commit.
# ------------------------------------------------------------------
test_expect_success '--all names every reachable commit' '
	cd repo &&
	A=$(cat .oid_A) &&
	B=$(cat .oid_B) &&
	C=$(cat .oid_C) &&
	{
		git name-rev "$A" &&
		git name-rev "$B" &&
		git name-rev "$C"
	} | sort >expect &&
	git name-rev --all | sort >actual &&
	test_cmp expect actual
'

# ------------------------------------------------------------------
# 11. --annotate-stdin annotates OIDs embedded in text.
# ------------------------------------------------------------------
test_expect_success '--annotate-stdin annotates OIDs in text' '
	cd repo &&
	C=$(cat .oid_C) &&
	NAME=$(git name-rev --name-only "$C") &&
	printf "%s (%s)\n" "$C" "$NAME" >expect &&
	printf "%s\n" "$C" | git name-rev --annotate-stdin >actual &&
	test_cmp expect actual
'

# ------------------------------------------------------------------
# 12. --refs=<pattern> restricts naming to matching refs.
#     When the pattern matches a sub-path (e.g. "v*" matches "v1.0" within
#     "refs/tags/v1.0") the name is shortened to the matched sub-path.
# ------------------------------------------------------------------
test_expect_success '--refs=v* limits to v1.0 tag and abbreviates sub-path match' '
	cd repo &&
	A=$(cat .oid_A) &&
	printf "%s v1.0\n" "$A" >expect &&
	git name-rev --refs="v*" "$A" >actual &&
	test_cmp expect actual
'

# ------------------------------------------------------------------
# 13. Merge commit: second parent gets ^2 suffix.
# ------------------------------------------------------------------
test_expect_success 'second parent of merge gets ^2 suffix' '
	cd repo &&
	EMPTY_TREE=$(cat .empty_tree) &&
	C=$(cat .oid_C) &&
	GIT_COMMITTER_DATE="1000003 +0000" GIT_AUTHOR_DATE="1000003 +0000" \
		D=$(git commit-tree "$EMPTY_TREE" -m "commit D") &&
	GIT_COMMITTER_DATE="1000004 +0000" GIT_AUTHOR_DATE="1000004 +0000" \
		M=$(git commit-tree "$EMPTY_TREE" -p "$C" -p "$D" -m "merge") &&
	git update-ref refs/heads/main "$M" &&

	# D is the second parent of M (which is main); D should be named main^2.
	printf "%s main^2\n" "$D" >expect &&
	git name-rev "$D" >actual &&
	test_cmp expect actual
'

test_done
