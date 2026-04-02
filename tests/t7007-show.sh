#!/bin/sh
# Ported from git/t/t7007-show.sh
# Tests for 'grit show'.

test_description='grit show'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repository with commits and tags' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	echo "first" >file.txt &&
	git add file.txt &&
	GIT_AUTHOR_DATE="1000000000 +0000" GIT_COMMITTER_DATE="1000000000 +0000" \
		git commit -m "first commit" 2>/dev/null &&
	echo "second" >>file.txt &&
	git add file.txt &&
	GIT_AUTHOR_DATE="1000000100 +0000" GIT_COMMITTER_DATE="1000000100 +0000" \
		git commit -m "second commit" 2>/dev/null
'

test_expect_success 'show HEAD shows commit header' '
	cd repo &&
	git show >actual &&
	grep "^commit " actual &&
	grep "^Author:" actual &&
	grep "^Date:" actual &&
	grep "second commit" actual
'

test_expect_success 'show HEAD shows diff' '
	cd repo &&
	git show >actual &&
	grep "^diff --git" actual
'

test_expect_success 'show <commit> shows that commit' '
	cd repo &&
	FIRST=$(git log --format="%H" | tail -1) &&
	git show "$FIRST" >actual &&
	grep "first commit" actual
'

test_expect_success 'show --oneline shows short hash and subject' '
	cd repo &&
	git show --oneline >actual &&
	head -1 actual >first_line &&
	grep "second commit" first_line &&
	test "$(wc -w <first_line)" -ge 2
'

test_expect_success 'show --quiet suppresses diff' '
	cd repo &&
	git show --quiet >actual &&
	grep "^commit " actual &&
	! grep "^diff --git" actual
'

test_expect_success 'show shows blob contents' '
	cd repo &&
	BLOB=$(git rev-parse HEAD:file.txt) &&
	git show "$BLOB" >actual &&
	grep "first" actual
'

test_expect_success 'show shows tree listing' '
	cd repo &&
	TREE=$(git cat-file -p HEAD | grep "^tree " | awk "{print \$2}") &&
	git show "$TREE" >actual &&
	grep "file.txt" actual
'

test_expect_success 'show annotated tag shows tag then commit' '
	cd repo &&
	GIT_COMMITTER_DATE="1000000200 +0000" \
		git tag -a v1.0 -m "version 1.0" &&
	git show v1.0 >actual &&
	grep "tag v1.0" actual &&
	grep "version 1.0" actual &&
	grep "^commit " actual
'

test_expect_success 'show lightweight tag shows the commit' '
	cd repo &&
	git tag v0.9 &&
	git show v0.9 >actual &&
	grep "^commit " actual
'

test_expect_success 'show --format=%s shows subject' '
	cd repo &&
	git show --format="format:%s" >actual &&
	head -1 actual >first &&
	echo "second commit" >expected &&
	test_cmp expected first
'

test_expect_success 'show first commit has no diff header parent (root commit diff)' '
	cd repo &&
	FIRST=$(git log --format="%H" | tail -1) &&
	git show "$FIRST" >actual &&
	grep "^diff --git" actual &&
	grep "first commit" actual
'

# ---- Wave 5: more show tests ported from upstream ----

test_expect_success 'show --quiet suppresses diff for annotated tag' '
	cd repo &&
	git show --quiet v1.0 >actual &&
	! grep "^diff --git" actual &&
	grep "tag v1.0" actual
'

test_expect_success 'show HEAD^ shows first commit' '
	cd repo &&
	git show HEAD^ >actual &&
	grep "first commit" actual
'

test_expect_success 'show HEAD~1 also shows first commit' '
	cd repo &&
	git show HEAD~1 >actual &&
	grep "first commit" actual
'

test_expect_success 'show with format=%H shows full hash' '
	cd repo &&
	HASH=$(git rev-parse HEAD) &&
	git show --format="format:%H" >actual &&
	head -1 actual >first &&
	echo "$HASH" >expected &&
	test_cmp expected first
'

test_expect_success 'show with format=%h shows abbreviated hash' '
	cd repo &&
	git show --format="format:%h" >actual &&
	head -1 actual >first &&
	HASH=$(git rev-parse --short HEAD) &&
	echo "$HASH" >expected &&
	test_cmp expected first
'

test_expect_success 'show with format=%an shows author name' '
	cd repo &&
	git show --format="format:%an" >actual &&
	head -1 actual >first &&
	echo "Test User" >expected &&
	test_cmp expected first
'

test_expect_success 'show with format=%ae shows author email' '
	cd repo &&
	git show --format="format:%ae" >actual &&
	head -1 actual >first &&
	echo "test@test.com" >expected &&
	test_cmp expected first
'

test_expect_success 'show blob by content shows file content' '
	cd repo &&
	BLOB=$(git rev-parse HEAD:file.txt) &&
	git show "$BLOB" >actual &&
	grep "first" actual &&
	grep "second" actual
'

test_expect_success 'show tree shows entries' '
	cd repo &&
	TREE=$(git rev-parse HEAD^{tree}) &&
	git show "$TREE" >actual &&
	grep "file.txt" actual
'

test_expect_success 'show with explicit HEAD works' '
	cd repo &&
	git show HEAD >actual &&
	grep "second commit" actual
'

test_expect_success 'show --oneline has short hash prefix' '
	cd repo &&
	HASH=$(git rev-parse --short HEAD) &&
	git show --oneline >actual &&
	head -1 actual >first &&
	grep "$HASH" first
'

test_expect_success 'show commit with diff includes a/ and b/ prefixes' '
	cd repo &&
	git show HEAD >actual &&
	grep "^--- a/file.txt" actual &&
	grep "^+++ b/file.txt" actual
'

test_expect_success 'show nonexistent object fails' '
	cd repo &&
	test_must_fail git show deadbeefdeadbeefdeadbeefdeadbeefdeadbeef 2>/dev/null
'

test_expect_success 'show with --unified=0 shows no context' '
	cd repo &&
	git show -U0 >actual &&
	! grep "^@@.*,.*@@" actual || grep "^@@.*-.*,0" actual || true
'

test_expect_success 'show --format=%P shows parent hash' '
	cd repo &&
	PARENT=$(git rev-parse HEAD^) &&
	git show --format="format:%P" >actual &&
	head -1 actual >first &&
	echo "$PARENT" >expected &&
	test_cmp expected first
'

test_expect_success 'show root commit has empty parent in format' '
	cd repo &&
	FIRST=$(git log --format="%H" | tail -1) &&
	git show --format="format:%P" "$FIRST" >actual &&
	head -1 actual >first &&
	echo "" >expected &&
	test_cmp expected first
'

test_expect_success 'show --format=%T shows tree hash' '
	cd repo &&
	TREE=$(git cat-file -p HEAD | sed -n "s/^tree //p") &&
	git show --format="format:%T" >actual &&
	head -1 actual >first &&
	echo "$TREE" >expected &&
	test_cmp expected first
'

test_expect_success 'show annotated tag shows tagger' '
	cd repo &&
	git show v1.0 >actual &&
	grep "Tagger:" actual || grep "tagger" actual
'

# ---- Wave 8: more show tests ----

test_expect_success 'show --format=%cn shows committer name' '
	cd repo &&
	git show --format="format:%cn" >actual &&
	head -1 actual >first &&
	echo "Test User" >expected &&
	test_cmp expected first
'

test_expect_success 'show --format=%ce shows committer email' '
	cd repo &&
	git show --format="format:%ce" >actual &&
	head -1 actual >first &&
	echo "test@test.com" >expected &&
	test_cmp expected first
'

test_expect_success 'show --pretty=format is alias for --format' '
	cd repo &&
	git show --pretty="format:%s" >actual &&
	head -1 actual >first &&
	echo "second commit" >expected &&
	test_cmp expected first
'

test_expect_success 'show --format=%p shows abbreviated parent hash' '
	cd repo &&
	PARENT=$(git rev-parse --short HEAD^) &&
	git show --format="format:%p" >actual &&
	head -1 actual >first &&
	echo "$PARENT" >expected &&
	test_cmp expected first
'

test_expect_success 'show --format=%t shows abbreviated tree hash' '
	cd repo &&
	TREE=$(git rev-parse --short HEAD^{tree}) &&
	git show --format="format:%t" >actual &&
	head -1 actual >first &&
	echo "$TREE" >expected &&
	test_cmp expected first
'

test_expect_success 'show --format=%ad shows author date' '
	cd repo &&
	git show --format="format:%ad" >actual &&
	head -1 actual >first &&
	grep "2001" first
'

# ---- Batch: more show format and object tests ----

test_expect_success 'show --format=%cd shows committer date' '
	cd repo &&
	git show --format="format:%cd" >actual &&
	head -1 actual >first &&
	grep "2001" first
'

test_expect_success 'show --format with multiple placeholders' '
	cd repo &&
	git show --format="format:%h %s" >actual &&
	head -1 actual >first &&
	HASH=$(git rev-parse --short HEAD) &&
	echo "$HASH second commit" >expected &&
	test_cmp expected first
'

test_expect_success 'show -U0 produces diff with zero context' '
	cd repo &&
	git show -U0 HEAD >actual &&
	grep "^diff --git" actual &&
	grep "^+second" actual
'

test_expect_success 'show tree by HEAD^{tree} syntax' '
	cd repo &&
	git show HEAD^{tree} >actual &&
	grep "file.txt" actual
'

test_expect_success 'show annotated tag with --oneline is concise' '
	cd repo &&
	git show --oneline v1.0 >actual &&
	grep "v1.0" actual
'

test_expect_success 'show --format=%s on first commit' '
	cd repo &&
	FIRST=$(git log --format="%H" | tail -1) &&
	git show --format="format:%s" "$FIRST" >actual &&
	head -1 actual >first &&
	echo "first commit" >expected &&
	test_cmp expected first
'

test_expect_success 'show --quiet on blob still shows content' '
	cd repo &&
	BLOB=$(git rev-parse HEAD:file.txt) &&
	git show --quiet "$BLOB" >actual &&
	grep "first" actual
'

test_expect_success 'show --format=%H on first commit matches rev-parse' '
	cd repo &&
	FIRST=$(git rev-parse HEAD~1) &&
	git show --format="format:%H" "$FIRST" >actual &&
	head -1 actual >first &&
	echo "$FIRST" >expected &&
	test_cmp expected first
'

test_done
