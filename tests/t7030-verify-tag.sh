#!/bin/sh
# Tests for tag verification (without GPG): tag creation, structure,
# cat-file inspection, mktag validation, and tag properties.

test_description='tag verification (basic checks, no GPG)'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ── Setup ────────────────────────────────────────────────────────────────────

test_expect_success 'setup repository with commits' '
	git init repo &&
	cd repo &&
	git config user.name "Tag Tester" &&
	git config user.email "tag@test.com" &&
	echo "first" >file.txt &&
	git add file.txt &&
	git commit -m "first commit" &&
	echo "second" >>file.txt &&
	git add file.txt &&
	git commit -m "second commit"
'

# ── Lightweight tag structure ───────────────────────────────────────────────

test_expect_success 'lightweight tag points directly to commit' '
	cd repo &&
	git tag v1.0 &&
	tag_oid=$(git rev-parse v1.0) &&
	head_oid=$(git rev-parse HEAD) &&
	test "$tag_oid" = "$head_oid"
'

test_expect_success 'lightweight tag type via rev-parse is commit' '
	cd repo &&
	type=$(git cat-file -t v1.0) &&
	test "$type" = "commit"
'

test_expect_success 'show-ref lists lightweight tag' '
	cd repo &&
	git show-ref --verify refs/tags/v1.0 >actual &&
	test_line_count = 1 actual
'

test_expect_success 'show-ref OID matches HEAD for lightweight tag' '
	cd repo &&
	ref_oid=$(git show-ref --verify refs/tags/v1.0 | cut -d" " -f1) &&
	head_oid=$(git rev-parse HEAD) &&
	test "$ref_oid" = "$head_oid"
'

# ── Annotated tag structure ─────────────────────────────────────────────────

test_expect_success 'annotated tag creates a tag object' '
	cd repo &&
	git tag -a -m "Release 2.0" v2.0 &&
	type=$(git cat-file -t v2.0) &&
	test "$type" = "tag"
'

test_expect_success 'annotated tag object has correct object field' '
	cd repo &&
	git cat-file tag v2.0 >raw &&
	head_oid=$(git rev-parse HEAD) &&
	grep "^object $head_oid$" raw
'

test_expect_success 'annotated tag object has type commit' '
	cd repo &&
	git cat-file tag v2.0 >raw &&
	grep "^type commit$" raw
'

test_expect_success 'annotated tag object has tag name' '
	cd repo &&
	git cat-file tag v2.0 >raw &&
	grep "^tag v2.0$" raw
'

test_expect_success 'annotated tag object has tagger line' '
	cd repo &&
	git cat-file tag v2.0 >raw &&
	grep "^tagger Tag Tester <tag@test.com>" raw
'

test_expect_success 'annotated tag object has message' '
	cd repo &&
	git cat-file tag v2.0 >raw &&
	grep "Release 2.0" raw
'

test_expect_success 'cat-file -s on annotated tag returns nonzero size' '
	cd repo &&
	size=$(git cat-file -s v2.0) &&
	test "$size" -gt 0
'

# ── Tag on specific commit ─────────────────────────────────────────────────

test_expect_success 'tag on specific earlier commit' '
	cd repo &&
	first_oid=$(git rev-parse HEAD~1) &&
	git tag v0.1 "$first_oid" &&
	tag_oid=$(git rev-parse v0.1) &&
	test "$tag_oid" = "$first_oid"
'

test_expect_success 'annotated tag on earlier commit references correct object' '
	cd repo &&
	first_oid=$(git rev-parse HEAD~1) &&
	git tag -a -m "Old release" v0.1a "$first_oid" &&
	git cat-file tag v0.1a >raw &&
	grep "^object $first_oid$" raw
'

# ── mktag validates tag structure ───────────────────────────────────────────

test_expect_success 'mktag accepts well-formed tag' '
	cd repo &&
	head_oid=$(git rev-parse HEAD) &&
	cat >tag-input <<-EOF &&
	object $head_oid
	type commit
	tag test-mktag
	tagger Test <test@test.com> 1234567890 +0000

	mktag test message
	EOF
	oid=$(git mktag <tag-input) &&
	test -n "$oid" &&
	type=$(git cat-file -t "$oid") &&
	test "$type" = "tag"
'

test_expect_success 'mktag rejects missing object line' '
	cd repo &&
	cat >bad-tag <<-EOF &&
	type commit
	tag bad-tag
	tagger Test <test@test.com> 1234567890 +0000

	bad
	EOF
	test_must_fail git mktag <bad-tag
'

test_expect_success 'mktag rejects missing type line' '
	cd repo &&
	head_oid=$(git rev-parse HEAD) &&
	cat >bad-tag <<-EOF &&
	object $head_oid
	tag bad-tag
	tagger Test <test@test.com> 1234567890 +0000

	bad
	EOF
	test_must_fail git mktag <bad-tag
'

test_expect_success 'mktag rejects missing tag name' '
	cd repo &&
	head_oid=$(git rev-parse HEAD) &&
	cat >bad-tag <<-EOF &&
	object $head_oid
	type commit
	tagger Test <test@test.com> 1234567890 +0000

	bad
	EOF
	test_must_fail git mktag <bad-tag
'

test_expect_success 'mktag rejects invalid object hash' '
	cd repo &&
	cat >bad-tag <<-EOF &&
	object 0000000000000000000000000000000000000000
	type commit
	tag bad-hash
	tagger Test <test@test.com> 1234567890 +0000

	bad hash
	EOF
	test_must_fail git mktag <bad-tag
'

# ── Tag deletion ────────────────────────────────────────────────────────────

test_expect_success 'delete lightweight tag' '
	cd repo &&
	git tag delete-me &&
	git tag -d delete-me &&
	! git show-ref --verify refs/tags/delete-me 2>/dev/null
'

test_expect_success 'delete annotated tag' '
	cd repo &&
	git tag -a -m "will delete" delete-anno &&
	git tag -d delete-anno &&
	! git show-ref --verify refs/tags/delete-anno 2>/dev/null
'

# ── Tag listing and filtering ──────────────────────────────────────────────

test_expect_success 'tag -l lists all tags' '
	cd repo &&
	git tag -l >actual &&
	grep v1.0 actual &&
	grep v2.0 actual &&
	grep v0.1 actual
'

test_expect_success 'tag -l with pattern filters' '
	cd repo &&
	git tag -l "v1*" >actual &&
	echo "v1.0" >expect &&
	test_cmp expect actual
'

test_expect_success 'tag -l with v2* pattern' '
	cd repo &&
	git tag -l "v2*" >actual &&
	echo "v2.0" >expect &&
	test_cmp expect actual
'

# ── Tag force overwrite ────────────────────────────────────────────────────

test_expect_success 'tag -f overwrites existing lightweight tag' '
	cd repo &&
	first_oid=$(git rev-parse HEAD~1) &&
	git tag -f v1.0 "$first_oid" &&
	tag_oid=$(git rev-parse v1.0) &&
	test "$tag_oid" = "$first_oid"
'

test_expect_success 'creating duplicate tag without -f fails' '
	cd repo &&
	test_must_fail git tag v2.0
'

# ── show on tag ─────────────────────────────────────────────────────────────

test_expect_success 'show on annotated tag displays tag info' '
	cd repo &&
	git show v2.0 >actual &&
	grep "tag v2.0" actual &&
	grep "Release 2.0" actual
'

test_expect_success 'show on lightweight tag displays commit' '
	cd repo &&
	git show v0.1 >actual &&
	grep "commit" actual
'

# ── Tag with -n shows annotation lines ──────────────────────────────────────

test_expect_success 'tag -n lists tags with annotation' '
	cd repo &&
	git tag -n >actual &&
	grep "v2.0" actual &&
	grep "Release 2.0" actual
'

test_done
