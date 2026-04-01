#!/bin/sh
# Tests for 'gust tag'.
# Ported from git/t/t7004-tag.sh

test_description='gust tag'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

tag_exists () {
	git show-ref --quiet --verify "refs/tags/$1"
}

# Setup a repo used throughout the tests
test_expect_success 'setup repository' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'listing all tags in an empty tree should succeed' '
	cd repo &&
	git tag -l
'

test_expect_success 'listing all tags in an empty tree should output nothing' '
	cd repo &&
	test $(git tag -l | wc -l) -eq 0 &&
	test $(git tag | wc -l) -eq 0
'

test_expect_success 'creating a tag in an empty tree should fail' '
	cd repo &&
	! git tag mynotag &&
	! tag_exists mynotag
'

test_expect_success 'creating a tag for an unknown revision should fail' '
	cd repo &&
	! git tag mytagnorev aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
'

test_expect_success 'setup: create first commit' '
	cd repo &&
	echo foo >foo &&
	git add foo &&
	git commit -m "Foo"
'

test_expect_success 'creating a tag using default HEAD should succeed' '
	cd repo &&
	git tag mytag &&
	tag_exists mytag
'

test_expect_success 'HEAD is forbidden as a tagname' '
	cd repo &&
	! git tag HEAD
'

test_expect_success 'listing all tags if one exists should succeed' '
	cd repo &&
	git tag -l &&
	git tag
'

test_expect_success 'listing all tags if one exists should output that tag' '
	cd repo &&
	test $(git tag -l) = mytag &&
	test $(git tag) = mytag
'

test_expect_success 'listing a tag using a matching pattern should output that tag' '
	cd repo &&
	test $(git tag -l mytag) = mytag
'

test_expect_success 'listing tags using a non-matching pattern should output nothing' '
	cd repo &&
	test $(git tag -l xxx | wc -l) -eq 0
'

test_expect_success 'trying to create a tag with the name of one existing should fail' '
	cd repo &&
	! git tag mytag
'

test_expect_success 'creating a tag using HEAD directly should succeed' '
	cd repo &&
	git tag myhead HEAD &&
	tag_exists myhead
'

test_expect_success '--force can create a tag with the name of one existing' '
	cd repo &&
	tag_exists mytag &&
	git tag --force mytag &&
	tag_exists mytag
'

test_expect_success 'trying to delete an unknown tag should fail' '
	cd repo &&
	! tag_exists unknown-tag &&
	! git tag -d unknown-tag
'

test_expect_success 'deleting an existing tag should succeed' '
	cd repo &&
	git tag to-delete &&
	tag_exists to-delete &&
	git tag -d to-delete &&
	! tag_exists to-delete
'

test_expect_success 'listing all tags should print them ordered' '
	cd repo &&
	git tag v1.0.1 &&
	git tag t211 &&
	git tag aa1 &&
	git tag v0.2.1 &&
	git tag v1.1.3 &&
	git tag cba &&
	git tag a1 &&
	git tag v1.0 &&
	git tag t210 &&
	cat >expect <<-\EOF &&
	a1
	aa1
	cba
	myhead
	mytag
	t210
	t211
	v0.2.1
	v1.0
	v1.0.1
	v1.1.3
	EOF
	git tag -l >actual &&
	test_cmp expect actual
'

test_expect_success 'listing tags with substring as pattern must print those matching' '
	cd repo &&
	cat >expect <<-\EOF &&
	a1
	aa1
	cba
	myhead
	mytag
	EOF
	git tag -l "*a*" >actual &&
	test_cmp expect actual
'

test_expect_success 'listing tags with a suffix as pattern must print those matching' '
	cd repo &&
	cat >expect <<-\EOF &&
	v0.2.1
	v1.0.1
	EOF
	git tag -l "*.1" >actual &&
	test_cmp expect actual
'

test_expect_success 'listing tags with a prefix as pattern must print those matching' '
	cd repo &&
	cat >expect <<-\EOF &&
	t210
	t211
	EOF
	git tag -l "t21*" >actual &&
	test_cmp expect actual
'

test_expect_success 'listing tags with ? in the pattern should print those matching' '
	cd repo &&
	cat >expect <<-\EOF &&
	v1.0.1
	v1.1.3
	EOF
	git tag -l "v1.?.?" >actual &&
	test_cmp expect actual
'

test_expect_success 'listing tags using v* should print only those having v' '
	cd repo &&
	cat >expect <<-\EOF &&
	v0.2.1
	v1.0
	v1.0.1
	v1.1.3
	EOF
	git tag -l "v*" >actual &&
	test_cmp expect actual
'

test_expect_success 'a lightweight tag should point to HEAD commit' '
	cd repo &&
	git tag non-annotated-tag &&
	test $(git cat-file -t non-annotated-tag) = commit &&
	test $(git rev-parse non-annotated-tag) = $(git rev-parse HEAD)
'

test_expect_success 'creating an annotated tag with -m should succeed' '
	cd repo &&
	git tag -m "A message" annotated-tag &&
	tag_exists annotated-tag &&
	test $(git cat-file -t annotated-tag) = tag
'

test_expect_success 'annotated tag object has correct fields' '
	cd repo &&
	git cat-file tag annotated-tag >actual &&
	grep "^object " actual &&
	grep "^type commit" actual &&
	grep "^tag annotated-tag" actual &&
	grep "^tagger " actual &&
	grep "A message" actual
'

test_expect_success 'creating an annotated tag with -F messagefile should succeed' '
	cd repo &&
	cat >msgfile <<-\EOF &&
	Another message
	in a file.
	EOF
	git tag -F msgfile file-annotated-tag &&
	tag_exists file-annotated-tag &&
	git cat-file tag file-annotated-tag | grep "Another message"
'

test_expect_success 'listing tags with -n shows annotation line' '
	cd repo &&
	git tag -n >actual &&
	grep "annotated-tag" actual
'

test_expect_success 'deleting an annotated tag should succeed' '
	cd repo &&
	git tag -d annotated-tag &&
	! tag_exists annotated-tag
'

test_expect_success 'tag pointing to a specific commit' '
	cd repo &&
	echo bar >bar &&
	git add bar &&
	git commit -m "Bar" &&
	head=$(git rev-parse HEAD) &&
	git tag tagged-head &&
	test $(git rev-parse tagged-head) = "$head"
'

test_done
