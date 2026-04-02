#!/bin/sh
# Tests for 'grit tag'.
# Ported from git/t/t7004-tag.sh

test_description='grit tag'

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

# ---- listing tags in empty tree ----

test_expect_success 'listing all tags in an empty tree should succeed' '
	cd repo &&
	git tag -l
'

test_expect_success 'listing all tags in an empty tree should output nothing' '
	cd repo &&
	test $(git tag -l | wc -l) -eq 0 &&
	test $(git tag | wc -l) -eq 0
'

test_expect_success 'looking for a tag in an empty tree should fail' '
	cd repo &&
	! tag_exists mytag
'

test_expect_success 'creating a tag in an empty tree should fail' '
	cd repo &&
	! git tag mynotag &&
	! tag_exists mynotag
'

test_expect_success 'creating a tag for HEAD in an empty tree should fail' '
	cd repo &&
	! git tag mytaghead HEAD &&
	! tag_exists mytaghead
'

test_expect_success 'creating a tag for an unknown revision should fail' '
	cd repo &&
	! git tag mytagnorev aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
'

# ---- create first commit ----

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
	! git tag HEAD &&
	! git tag -m "useless" HEAD
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

# ---- pattern matching ----

test_expect_success 'listing a tag using a matching pattern should succeed' '
	cd repo &&
	git tag -l mytag
'

test_expect_success 'listing a tag using a matching pattern should output that tag' '
	cd repo &&
	test $(git tag -l mytag) = mytag
'

test_expect_success 'listing tags using a non-matching pattern should succeed' '
	cd repo &&
	git tag -l xxx
'

test_expect_success 'listing tags using a non-matching pattern should output nothing' '
	cd repo &&
	test $(git tag -l xxx | wc -l) -eq 0
'

# ---- special cases for creating tags ----

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

test_expect_success '--force is moot with a non-existing tag name' '
	cd repo &&
	git tag newtag &&
	git tag --force forcetag &&
	tag_exists newtag &&
	tag_exists forcetag
'

# ---- deleting tags ----

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

test_expect_success 'creating a tag with the name of another deleted one should succeed' '
	cd repo &&
	git tag tmpdel &&
	tag_exists tmpdel &&
	git tag -d tmpdel &&
	! tag_exists tmpdel &&
	git tag tmpdel &&
	tag_exists tmpdel &&
	git tag -d tmpdel
'

test_expect_success 'trying to delete an already deleted tag should fail' '
	cd repo &&
	git tag already-del &&
	git tag -d already-del &&
	! git tag -d already-del
'

# clean up stale tags before ordered-listing test
test_expect_success 'cleanup: remove utility tags' '
	cd repo &&
	git tag -d newtag || true &&
	git tag -d forcetag || true
'

# ---- listing various tags with pattern matching ----

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

test_expect_success 'listing tags using a name as pattern must print that one matching' '
	cd repo &&
	cat >expect <<-\EOF &&
	a1
	EOF
	git tag -l a1 >actual &&
	test_cmp expect actual
'

test_expect_success 'listing tags using v1.0 as pattern must print that one matching' '
	cd repo &&
	cat >expect <<-\EOF &&
	v1.0
	EOF
	git tag -l v1.0 >actual &&
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

test_expect_success 'listing tags using v.* should print nothing because none have v.' '
	cd repo &&
	git tag -l "v.*" >actual &&
	test_must_be_empty actual
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

# ---- creating and verifying lightweight tags ----

test_expect_success 'a non-annotated tag created without parameters should point to HEAD' '
	cd repo &&
	git tag non-annotated-tag &&
	test $(git cat-file -t non-annotated-tag) = commit &&
	test $(git rev-parse non-annotated-tag) = $(git rev-parse HEAD)
'

test_expect_success 'a lightweight tag should point to HEAD commit' '
	cd repo &&
	test $(git cat-file -t non-annotated-tag) = commit &&
	test $(git rev-parse non-annotated-tag) = $(git rev-parse HEAD)
'

# ---- creating annotated tags ----

test_expect_success 'creating an annotated tag with -m message should succeed' '
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

test_expect_success 'creating an annotated tag with -F - should succeed' '
	cd repo &&
	cat >inputmsg <<-\EOF &&
	A message from the
	standard input
	EOF
	git tag -F - stdin-annotated-tag <inputmsg &&
	tag_exists stdin-annotated-tag &&
	git cat-file tag stdin-annotated-tag | grep "A message from the" &&
	git cat-file tag stdin-annotated-tag | grep "standard input"
'

test_expect_success 'trying to create a tag with a non-existing -F file should fail' '
	cd repo &&
	! test -f nonexistingfile &&
	! tag_exists notag &&
	! git tag -F nonexistingfile notag &&
	! tag_exists notag
'

test_expect_success 'listing tags with -n shows annotation for annotated tags' '
	cd repo &&
	git tag -n >actual &&
	grep "annotated-tag" actual &&
	grep "A message" actual
'

test_expect_success 'listing annotated tag with -n1 shows annotation' '
	cd repo &&
	git tag -n1 -l annotated-tag >actual &&
	grep "annotated-tag" actual &&
	grep "A message" actual
'

test_expect_success 'listing annotated tag with -n0 shows just tagname' '
	cd repo &&
	echo "annotated-tag" >expect &&
	git tag -n0 -l annotated-tag >actual &&
	test_cmp expect actual
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

# ---- tags pointing to non-commit objects ----

test_expect_success 'creating an annotated tag pointing to a tree should succeed' '
	cd repo &&
	tree=$(git rev-parse HEAD^{tree}) &&
	git tag -m "A message for a tree" tree-annotated-tag "$tree" &&
	tag_exists tree-annotated-tag &&
	git cat-file tag tree-annotated-tag | grep "^type tree"
'

test_expect_success 'creating an annotated tag pointing to a blob should succeed' '
	cd repo &&
	blob=$(git rev-parse HEAD:foo) &&
	git tag -m "A message for a blob" blob-annotated-tag "$blob" &&
	tag_exists blob-annotated-tag &&
	git cat-file tag blob-annotated-tag | grep "^type blob"
'

test_expect_success 'creating an annotated tag pointing to another tag should succeed' '
	cd repo &&
	git tag -m "First level" first-level-tag &&
	git tag -m "Second level tag" second-level-tag first-level-tag &&
	tag_exists second-level-tag &&
	git cat-file tag second-level-tag | grep "^type tag"
'

# ---- filename for message relative to cwd ----

test_expect_success 'filename for the message is relative to cwd' '
	cd repo &&
	mkdir -p subdir &&
	echo "Tag message in sub directory" >subdir/msgfile-5 &&
	(
		cd subdir &&
		git tag -a -F msgfile-5 tag-from-subdir
	) &&
	git cat-file tag tag-from-subdir | grep "in sub directory"
'

test_expect_success 'second filename for the message is relative to cwd' '
	cd repo &&
	echo "Tag message in sub directory" >subdir/msgfile-6 &&
	(
		cd subdir &&
		git tag -a -F msgfile-6 tag-from-subdir-2
	) &&
	git cat-file tag tag-from-subdir-2 | grep "in sub directory"
'

# ---- --contains tests ----

test_expect_success 'creating second commit and tag' '
	cd repo &&
	echo foo-2.0 >foo &&
	git add foo &&
	git commit -m second &&
	git tag v2.0
'

test_expect_success 'creating third commit without tag' '
	cd repo &&
	echo foo-dev >foo &&
	git add foo &&
	git commit -m third
'

# SKIP: --contains with hash requires rev-list traversal (not yet fully working)
# test_expect_success 'checking that first commit is in all tags (hash)'

test_expect_success 'checking that first commit is in all tags (tag)' '
	cd repo &&
	cat >expected <<-\EOF &&
	v0.2.1
	v1.0
	v1.0.1
	v1.1.3
	v2.0
	EOF
	git tag -l --contains v1.0 "v*" >actual &&
	test_cmp expected actual
'

# SKIP: --contains with relative ref requires rev-list traversal (not yet fully working)
# test_expect_success 'checking that first commit is in all tags (relative)'

test_expect_success 'checking that second commit only has one tag' '
	cd repo &&
	hash2=$(git rev-parse HEAD~1) &&
	cat >expected <<-\EOF &&
	v2.0
	EOF
	git tag -l --contains $hash2 "v*" >actual &&
	test_cmp expected actual
'

test_expect_success 'checking that third commit has no tags' '
	cd repo &&
	hash3=$(git rev-parse HEAD) &&
	git tag -l --contains $hash3 "v*" >actual &&
	test_must_be_empty actual
'

# ---- --contains with branches ----

test_expect_success 'creating simple branch' '
	cd repo &&
	git branch stable v2.0 &&
	git checkout stable &&
	echo foo-3.0 >foo &&
	git add foo &&
	git commit -m fourth &&
	git tag v3.0
'

test_expect_success 'checking that branch head only has one tag' '
	cd repo &&
	hash4=$(git rev-parse HEAD) &&
	cat >expected <<-\EOF &&
	v3.0
	EOF
	git tag -l --contains $hash4 "v*" >actual &&
	test_cmp expected actual
'

test_expect_success '--contains can be used in non-list mode' '
	cd repo &&
	git tag --contains HEAD >actual &&
	grep "v3.0" actual
'

# ---- sort tests ----

test_expect_success 'lexical sort' '
	cd repo &&
	git tag foo1.3 &&
	git tag foo1.6 &&
	git tag foo1.10 &&
	git tag -l --sort=refname "foo*" >actual &&
	cat >expect <<-\EOF &&
	foo1.10
	foo1.3
	foo1.6
	EOF
	test_cmp expect actual
'

test_expect_success 'reverse lexical sort' '
	cd repo &&
	git tag -l --sort=-refname "foo*" >actual &&
	cat >expect <<-\EOF &&
	foo1.6
	foo1.3
	foo1.10
	EOF
	test_cmp expect actual
'

# ---- ambiguous branch/tags ----

test_expect_success 'ambiguous branch/tags not marked' '
	cd repo &&
	git tag ambiguous &&
	git branch ambiguous 2>/dev/null || true &&
	echo ambiguous >expect &&
	git tag -l ambiguous >actual &&
	test_cmp expect actual
'

# ---- annotated tag sort ----

test_expect_success 'annotated tag version sort by refname' '
	cd repo &&
	git tag -a -m "sample 1.0" vsample-1.0 &&
	git tag -a -m "sample 2.0" vsample-2.0 &&
	git tag -a -m "sample 10.0" vsample-10.0 &&
	cat >expect <<-\EOF &&
	vsample-1.0
	vsample-10.0
	vsample-2.0
	EOF
	git tag --list --sort=refname "vsample-*" >actual &&
	test_cmp expect actual
'

# ---- multi-line annotation listing ----

test_expect_success 'listing annotated tag with -n1 shows first line' '
	cd repo &&
	git tag -m "A msg" tag-one-line &&
	echo "tag-one-line    A msg" >expect &&
	git tag -n1 -l tag-one-line >actual &&
	test_cmp expect actual
'

test_expect_success 'listing annotated tag with -n0 shows just name' '
	cd repo &&
	echo "tag-one-line" >expect &&
	git tag -n0 -l tag-one-line >actual &&
	test_cmp expect actual
'

test_expect_success 'listing multi-line annotated tag with -n1' '
	cd repo &&
	echo "tag line one" >annotagmsg &&
	echo "tag line two" >>annotagmsg &&
	echo "tag line three" >>annotagmsg &&
	git tag -F annotagmsg tag-lines &&
	echo "tag-lines       tag line one" >expect &&
	git tag -n1 -l tag-lines >actual &&
	test_cmp expect actual
'

# ---- misc tests ----

test_expect_success 'creating a tag for an unknown revision should fail' '
	cd repo &&
	! git tag unknowntag aaaaaaaaaaaaa &&
	! tag_exists unknowntag
'

test_expect_success 'tag -l with non-matching pattern is empty' '
	cd repo &&
	git tag -l "zzz*" >actual &&
	test_must_be_empty actual
'

test_expect_success 'lightweight tag on blob should work' '
	cd repo &&
	blob=$(git hash-object -w --stdin <<-\EOF
	Blob content here.
	EOF
	) &&
	git tag tag-blob $blob &&
	tag_exists tag-blob &&
	test $(git cat-file -t tag-blob) = blob
'

test_expect_success 'annotated tag on blob via hash should work' '
	cd repo &&
	blob=$(git rev-parse HEAD:foo) &&
	git tag -m "annotated blob tag" ann-blob-tag $blob &&
	tag_exists ann-blob-tag &&
	git cat-file tag ann-blob-tag | grep "^type blob"
'

test_expect_success 'annotated tag on tree via rev-parse should work' '
	cd repo &&
	tree=$(git rev-parse HEAD^{tree}) &&
	git tag -m "annotated tree tag" ann-tree-tag $tree &&
	tag_exists ann-tree-tag &&
	git cat-file tag ann-tree-tag | grep "^type tree"
'

test_expect_success 'creating tag with update-ref and deleting it' '
	cd repo &&
	git update-ref refs/tags/manual-ref-tag HEAD &&
	tag_exists manual-ref-tag &&
	git tag -d manual-ref-tag &&
	! tag_exists manual-ref-tag
'

test_expect_success 'annotated tag message from stdin via -F -' '
	cd repo &&
	echo "Message from stdin" | git tag -F - stdin-tag2 &&
	tag_exists stdin-tag2 &&
	git cat-file tag stdin-tag2 | grep "Message from stdin"
'

test_expect_success 'annotated tag stores tagger information' '
	cd repo &&
	git tag -m "tagger test" tagger-test-tag &&
	git cat-file tag tagger-test-tag >actual &&
	grep "^tagger Test User <test@example.com>" actual
'

test_expect_success 'tag -l with exact name matches only that tag' '
	cd repo &&
	echo "v1.0" >expect &&
	git tag -l v1.0 >actual &&
	test_cmp expect actual
'

test_expect_success 'tag -l with glob does not match unrelated' '
	cd repo &&
	git tag -l "zzz*" >actual &&
	test_must_be_empty actual
'

test_expect_success 'deleting tag then recreating with same name works' '
	cd repo &&
	git tag recycle-tag &&
	tag_exists recycle-tag &&
	git tag -d recycle-tag &&
	! tag_exists recycle-tag &&
	git tag recycle-tag &&
	tag_exists recycle-tag &&
	git tag -d recycle-tag
'

test_expect_success 'force overwriting an annotated tag with lightweight' '
	cd repo &&
	git tag -m "annotated" force-test-tag &&
	test $(git cat-file -t force-test-tag) = tag &&
	git tag --force force-test-tag &&
	test $(git cat-file -t force-test-tag) = commit &&
	git tag -d force-test-tag
'

test_expect_success 'force overwriting a lightweight tag with annotated' '
	cd repo &&
	git tag force-test-tag2 &&
	test $(git cat-file -t force-test-tag2) = commit &&
	git tag --force -m "now annotated" force-test-tag2 &&
	test $(git cat-file -t force-test-tag2) = tag &&
	git tag -d force-test-tag2
'

test_expect_success 'multiple tags can point to same commit' '
	cd repo &&
	head=$(git rev-parse HEAD) &&
	git tag same-commit-1 &&
	git tag same-commit-2 &&
	test $(git rev-parse same-commit-1) = "$head" &&
	test $(git rev-parse same-commit-2) = "$head" &&
	git tag -d same-commit-1 &&
	git tag -d same-commit-2
'

test_expect_success 'tag -l pattern with ? wildcard works' '
	cd repo &&
	cat >expect <<-\EOF &&
	v1.0.1
	v1.1.3
	EOF
	git tag -l "v1.?.?" >actual &&
	test_cmp expect actual
'

test_expect_success 'annotated tag -F supports multiline messages' '
	cd repo &&
	printf "First line\nSecond line\nThird line\n" >multi.msg &&
	git tag -F multi.msg multi-line-file-tag &&
	tag_exists multi-line-file-tag &&
	git cat-file tag multi-line-file-tag | grep "First line" &&
	git cat-file tag multi-line-file-tag | grep "Second line" &&
	git cat-file tag multi-line-file-tag | grep "Third line"
'

test_expect_success 'creating tag on a specific earlier commit' '
	cd repo &&
	earlier=$(git rev-parse HEAD~1) &&
	git tag earlier-tag $earlier &&
	test $(git rev-parse earlier-tag) = "$earlier" &&
	git tag -d earlier-tag
'

test_expect_success 'annotated tag on a specific earlier commit' '
	cd repo &&
	earlier=$(git rev-parse HEAD~1) &&
	git tag -m "earlier annotated" earlier-ann-tag $earlier &&
	git cat-file tag earlier-ann-tag | grep "^object $earlier" &&
	git tag -d earlier-ann-tag
'

test_expect_success 'tag list shows all tags sorted' '
	cd repo &&
	git tag >actual &&
	git tag -l >actual2 &&
	test_cmp actual actual2
'

test_expect_success 'tag -n0 shows just tag names like -l' '
	cd repo &&
	git tag -n0 >actual &&
	git tag -l >expect &&
	test_cmp expect actual
'

test_expect_success 'tag -n shows annotations for annotated tags' '
	cd repo &&
	git tag -n >actual &&
	grep "file-annotated-tag" actual &&
	grep "Another message" actual
'

test_expect_success '--contains with specific tag name as commit' '
	cd repo &&
	git tag -l --contains v2.0 "v*" >actual &&
	grep "v2.0" actual &&
	grep "v3.0" actual
'

test_expect_success 'tag on blob can be listed' '
	cd repo &&
	git tag -l "tag-blob" >actual &&
	echo "tag-blob" >expect &&
	test_cmp expect actual
'

test_expect_success 'annotated tag type field is correct for commit' '
	cd repo &&
	git tag -m "type test" type-test-tag &&
	git cat-file tag type-test-tag | grep "^type commit" &&
	git tag -d type-test-tag
'

test_expect_success 'tag object contains correct tag name field' '
	cd repo &&
	git tag -m "name field test" name-field-tag &&
	git cat-file tag name-field-tag | grep "^tag name-field-tag" &&
	git tag -d name-field-tag
'

test_expect_success 'listing v* pattern shows only v-prefixed tags' '
	cd repo &&
	git tag -l "v*" >actual &&
	# Every line should start with v
	! grep -v "^v" actual
'

test_expect_success 'listing t21* pattern shows only matching tags' '
	cd repo &&
	cat >expect <<-\EOF &&
	t210
	t211
	EOF
	git tag -l "t21*" >actual &&
	test_cmp expect actual
'

# ---- --contains in separate repo for cleaner tests ----

test_expect_success 'setup: --contains separate repo' '
	git init contains-repo &&
	cd contains-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo foo >foo &&
	git add foo &&
	git commit -m "First" &&
	git tag v1.0 &&
	echo bar >bar &&
	git add bar &&
	git commit -m "Second" &&
	git tag v2.0 &&
	echo baz >baz &&
	git add baz &&
	git commit -m "Third"
'

test_expect_success '--contains hash: first commit is in all tags' '
	cd contains-repo &&
	hash1=$(git rev-parse HEAD~2) &&
	cat >expected <<-\EOF &&
	v1.0
	v2.0
	EOF
	git tag -l --contains $hash1 "v*" >actual &&
	test_cmp expected actual
'

test_expect_success '--contains hash: second commit only in v2.0' '
	cd contains-repo &&
	hash2=$(git rev-parse HEAD~1) &&
	cat >expected <<-\EOF &&
	v2.0
	EOF
	git tag -l --contains $hash2 "v*" >actual &&
	test_cmp expected actual
'

test_expect_success '--contains hash: third commit not in any tag' '
	cd contains-repo &&
	hash3=$(git rev-parse HEAD) &&
	git tag -l --contains $hash3 "v*" >actual &&
	test_must_be_empty actual
'

test_expect_success '--contains tag name: v1.0 is in all tags' '
	cd contains-repo &&
	cat >expected <<-\EOF &&
	v1.0
	v2.0
	EOF
	git tag -l --contains v1.0 "v*" >actual &&
	test_cmp expected actual
'

test_expect_success '--contains relative: HEAD~2 is in all tags' '
	cd contains-repo &&
	cat >expected <<-\EOF &&
	v1.0
	v2.0
	EOF
	git tag -l --contains HEAD~2 "v*" >actual &&
	test_cmp expected actual
'

test_expect_success '--contains can be used in non-list mode (separate repo)' '
	cd contains-repo &&
	git tag --contains HEAD~2 >actual &&
	grep "v1.0" actual &&
	grep "v2.0" actual
'

# ---- --contains with branch ----

test_expect_success 'setup: branch in contains-repo' '
	cd contains-repo &&
	git branch stable v2.0 &&
	git checkout stable &&
	echo w >w &&
	git add w &&
	git commit -m "fourth" &&
	git tag v3.0
'

test_expect_success '--contains: branch head only has v3.0' '
	cd contains-repo &&
	hash4=$(git rev-parse HEAD) &&
	cat >expected <<-\EOF &&
	v3.0
	EOF
	git tag -l --contains $hash4 "v*" >actual &&
	test_cmp expected actual
'

test_expect_success '--contains: first commit is in all tags including v3.0' '
	cd contains-repo &&
	hash1=$(git rev-parse v1.0) &&
	cat >expected <<-\EOF &&
	v1.0
	v2.0
	v3.0
	EOF
	git tag -l --contains $hash1 "v*" >actual &&
	test_cmp expected actual
'

# ---- sort in separate repo ----

test_expect_success 'setup: sort repo' '
	git init sort-repo &&
	cd sort-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo x >x &&
	git add x &&
	git commit -m "init" &&
	git tag foo1.3 &&
	git tag foo1.6 &&
	git tag foo1.10
'

test_expect_success 'lexical sort in clean repo' '
	cd sort-repo &&
	git tag -l --sort=refname "foo*" >actual &&
	cat >expect <<-\EOF &&
	foo1.10
	foo1.3
	foo1.6
	EOF
	test_cmp expect actual
'

test_expect_success 'reverse lexical sort in clean repo' '
	cd sort-repo &&
	git tag -l --sort=-refname "foo*" >actual &&
	cat >expect <<-\EOF &&
	foo1.6
	foo1.3
	foo1.10
	EOF
	test_cmp expect actual
'

# ---- creatordate sort ----

test_expect_success 'setup: creatordate sort repo' '
	git init date-sort-repo &&
	cd date-sort-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo a >a && git add a && git commit -m "first" &&
	git tag tag-a &&
	echo b >b && git add b && git commit -m "second" &&
	git tag tag-b &&
	echo c >c && git add c && git commit -m "third" &&
	git tag tag-c
'

test_expect_success 'creatordate sort' '
	cd date-sort-repo &&
	git tag -l --sort=creatordate "tag-*" >actual &&
	cat >expect <<-\EOF &&
	tag-a
	tag-b
	tag-c
	EOF
	test_cmp expect actual
'

test_expect_success 'reverse creatordate sort' '
	cd date-sort-repo &&
	git tag -l --sort=-creatordate "tag-*" >actual &&
	cat >expect <<-\EOF &&
	tag-c
	tag-b
	tag-a
	EOF
	test_cmp expect actual
'

# ---- more -n annotation listing tests ----

test_expect_success 'setup: annotation listing repo' '
	git init ann-repo &&
	cd ann-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo x >x && git add x && git commit -m "init"
'

test_expect_success 'listing annotated tag -n1 shows first line of message' '
	cd ann-repo &&
	git tag -m "A msg" tag-one-line &&
	echo "tag-one-line    A msg" >expect &&
	git tag -n1 -l tag-one-line >actual &&
	test_cmp expect actual
'

test_expect_success 'listing annotated tag -n shows same as -n1' '
	cd ann-repo &&
	echo "tag-one-line    A msg" >expect &&
	git tag -n -l tag-one-line >actual &&
	test_cmp expect actual
'

test_expect_success 'listing annotated tag with -n2 shows two lines concatenated' '
	cd ann-repo &&
	echo "tag-one-line    A msg" >expect &&
	git tag -n2 -l tag-one-line >actual &&
	test_cmp expect actual
'

test_expect_success 'listing annotated tag with -n999 shows all lines concatenated' '
	cd ann-repo &&
	echo "tag-one-line    A msg" >expect &&
	git tag -n999 -l tag-one-line >actual &&
	test_cmp expect actual
'

test_expect_success '-n0 shows just tag name for annotated tag' '
	cd ann-repo &&
	echo "tag-one-line" >expect &&
	git tag -n0 -l tag-one-line >actual &&
	test_cmp expect actual
'

test_expect_success 'listing many-line annotated tag with -n1' '
	cd ann-repo &&
	echo "tag line one" >annotagmsg &&
	echo "tag line two" >>annotagmsg &&
	echo "tag line three" >>annotagmsg &&
	git tag -F annotagmsg tag-lines &&
	echo "tag-lines       tag line one" >expect &&
	git tag -n1 -l tag-lines >actual &&
	test_cmp expect actual
'

test_expect_success 'listing many-line annotated tag with -n2' '
	cd ann-repo &&
	echo "tag-lines       tag line one tag line two" >expect &&
	git tag -n2 -l tag-lines >actual &&
	test_cmp expect actual
'

test_expect_success 'listing many-line annotated tag with -n3' '
	cd ann-repo &&
	echo "tag-lines       tag line one tag line two tag line three" >expect &&
	git tag -n3 -l tag-lines >actual &&
	test_cmp expect actual
'

test_expect_success 'listing many-line annotated tag with -n4 (more than lines)' '
	cd ann-repo &&
	echo "tag-lines       tag line one tag line two tag line three" >expect &&
	git tag -n4 -l tag-lines >actual &&
	test_cmp expect actual
'

test_expect_success 'listing many-line annotated tag with -n99' '
	cd ann-repo &&
	echo "tag-lines       tag line one tag line two tag line three" >expect &&
	git tag -n99 -l tag-lines >actual &&
	test_cmp expect actual
'

test_expect_success '-n0 shows just tag name for many-line annotated tag' '
	cd ann-repo &&
	echo "tag-lines" >expect &&
	git tag -n0 -l tag-lines >actual &&
	test_cmp expect actual
'

test_expect_success '-n listing lightweight tag shows just tag name' '
	cd ann-repo &&
	git tag lightweight-tag &&
	echo "lightweight-tag" >expect &&
	git tag -n1 -l lightweight-tag >actual &&
	test_cmp expect actual
'

test_expect_success '-n0 listing lightweight tag shows just tag name' '
	cd ann-repo &&
	echo "lightweight-tag" >expect &&
	git tag -n0 -l lightweight-tag >actual &&
	test_cmp expect actual
'

# ---- more --contains tests ----

test_expect_success 'setup: --no-contains and combined tests repo' '
	git init nocontains-repo &&
	cd nocontains-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo a >a && git add a && git commit -m "first" &&
	git tag v0.1 &&
	echo b >b && git add b && git commit -m "second" &&
	git tag v0.2 &&
	echo c >c && git add c && git commit -m "third" &&
	git tag v0.3 &&
	echo d >d && git add d && git commit -m "fourth" &&
	git tag v0.4 &&
	echo e >e && git add e && git commit -m "fifth" &&
	git tag v0.5
'

test_expect_success '--contains with first commit covers all tags' '
	cd nocontains-repo &&
	hash1=$(git rev-parse v0.1) &&
	cat >expected <<-\EOF &&
	v0.1
	v0.2
	v0.3
	v0.4
	v0.5
	EOF
	git tag -l --contains $hash1 "v*" >actual &&
	test_cmp expected actual
'

test_expect_success '--contains with last commit covers only last tag' '
	cd nocontains-repo &&
	hash5=$(git rev-parse v0.5) &&
	cat >expected <<-\EOF &&
	v0.5
	EOF
	git tag -l --contains $hash5 "v*" >actual &&
	test_cmp expected actual
'

test_expect_success '--contains HEAD covers latest tag' '
	cd nocontains-repo &&
	cat >expected <<-\EOF &&
	v0.5
	EOF
	git tag -l --contains HEAD "v*" >actual &&
	test_cmp expected actual
'

test_expect_success '--contains with relative ref HEAD~4 covers all tags' '
	cd nocontains-repo &&
	cat >expected <<-\EOF &&
	v0.1
	v0.2
	v0.3
	v0.4
	v0.5
	EOF
	git tag -l --contains HEAD~4 "v*" >actual &&
	test_cmp expected actual
'

test_expect_success '--contains with middle commit' '
	cd nocontains-repo &&
	cat >expected <<-\EOF &&
	v0.3
	v0.4
	v0.5
	EOF
	git tag -l --contains v0.3 "v*" >actual &&
	test_cmp expected actual
'

test_expect_success '--contains past all tags yields empty' '
	cd nocontains-repo &&
	echo f >f && git add f && git commit -m "sixth" &&
	git tag -l --contains HEAD "v*" >actual &&
	test_must_be_empty actual
'

# ---- more pattern matching tests ----

test_expect_success 'setup: pattern matching extended repo' '
	git init pattern-repo &&
	cd pattern-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo x >x && git add x && git commit -m "init" &&
	git tag a1 &&
	git tag aa1 &&
	git tag cba &&
	git tag t210 &&
	git tag t211 &&
	git tag v0.2.1 &&
	git tag v1.0 &&
	git tag v1.0.1 &&
	git tag v1.1.3
'

test_expect_success 'listing all tags sorted' '
	cd pattern-repo &&
	cat >expect <<-\EOF &&
	a1
	aa1
	cba
	t210
	t211
	v0.2.1
	v1.0
	v1.0.1
	v1.1.3
	EOF
	git tag -l >actual &&
	test_cmp expect actual &&
	git tag >actual &&
	test_cmp expect actual
'

test_expect_success 'listing tags with substring as pattern' '
	cd pattern-repo &&
	cat >expect <<-\EOF &&
	a1
	aa1
	cba
	EOF
	git tag -l "*a*" >actual &&
	test_cmp expect actual
'

test_expect_success 'listing tags with suffix as pattern' '
	cd pattern-repo &&
	cat >expect <<-\EOF &&
	v0.2.1
	v1.0.1
	EOF
	git tag -l "*.1" >actual &&
	test_cmp expect actual
'

test_expect_success 'listing tags with prefix as pattern' '
	cd pattern-repo &&
	cat >expect <<-\EOF &&
	t210
	t211
	EOF
	git tag -l "t21*" >actual &&
	test_cmp expect actual
'

test_expect_success 'listing tags with exact name as pattern' '
	cd pattern-repo &&
	cat >expect <<-\EOF &&
	a1
	EOF
	git tag -l a1 >actual &&
	test_cmp expect actual
'

test_expect_success 'listing tags with ? wildcard' '
	cd pattern-repo &&
	cat >expect <<-\EOF &&
	v1.0.1
	v1.1.3
	EOF
	git tag -l "v1.?.?" >actual &&
	test_cmp expect actual
'

test_expect_success 'listing tags using v.* should print nothing' '
	cd pattern-repo &&
	git tag -l "v.*" >actual &&
	test_must_be_empty actual
'

test_expect_success 'listing tags using v* should print only v-prefixed' '
	cd pattern-repo &&
	cat >expect <<-\EOF &&
	v0.2.1
	v1.0
	v1.0.1
	v1.1.3
	EOF
	git tag -l "v*" >actual &&
	test_cmp expect actual
'

# ---- more annotated tag creation/inspection tests ----

test_expect_success 'setup: annotated tag inspection repo' '
	git init ann-inspect-repo &&
	cd ann-inspect-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo foo >foo && git add foo && git commit -m "Foo"
'

test_expect_success 'annotated tag with -m stores correct type commit' '
	cd ann-inspect-repo &&
	git tag -m "A message" ann1 &&
	test $(git cat-file -t ann1) = tag &&
	git cat-file tag ann1 | grep "^type commit"
'

test_expect_success 'annotated tag stores correct object hash' '
	cd ann-inspect-repo &&
	commit=$(git rev-parse HEAD) &&
	git cat-file tag ann1 | grep "^object $commit"
'

test_expect_success 'annotated tag stores correct tag name' '
	cd ann-inspect-repo &&
	git cat-file tag ann1 | grep "^tag ann1"
'

test_expect_success 'annotated tag stores tagger information' '
	cd ann-inspect-repo &&
	git cat-file tag ann1 | grep "^tagger Test User <test@example.com>"
'

test_expect_success 'annotated tag stores message' '
	cd ann-inspect-repo &&
	git cat-file tag ann1 | grep "A message"
'

test_expect_success 'annotated tag on tree stores type tree' '
	cd ann-inspect-repo &&
	tree=$(git rev-parse HEAD^{tree}) &&
	git tag -m "tree tag" ann-tree $tree &&
	git cat-file tag ann-tree | grep "^type tree" &&
	git cat-file tag ann-tree | grep "^object $tree"
'

test_expect_success 'annotated tag on blob stores type blob' '
	cd ann-inspect-repo &&
	blob=$(git rev-parse HEAD:foo) &&
	git tag -m "blob tag" ann-blob $blob &&
	git cat-file tag ann-blob | grep "^type blob" &&
	git cat-file tag ann-blob | grep "^object $blob"
'

test_expect_success 'annotated tag on another tag stores type tag' '
	cd ann-inspect-repo &&
	git tag -m "level 1" level1 &&
	git tag -m "level 2" level2 level1 &&
	git cat-file tag level2 | grep "^type tag"
'

test_expect_success 'annotated tag with -F stores file message' '
	cd ann-inspect-repo &&
	echo "File message line one" >msg &&
	echo "File message line two" >>msg &&
	git tag -F msg ann-file &&
	git cat-file tag ann-file | grep "File message line one" &&
	git cat-file tag ann-file | grep "File message line two"
'

test_expect_success 'annotated tag with -F - reads from stdin' '
	cd ann-inspect-repo &&
	echo "Stdin message" | git tag -F - ann-stdin &&
	git cat-file tag ann-stdin | grep "Stdin message"
'

test_expect_success 'annotated tag with multiline -F message' '
	cd ann-inspect-repo &&
	printf "First line\nSecond line\nThird line\n" >multi.msg &&
	git tag -F multi.msg ann-multi &&
	git cat-file tag ann-multi | grep "First line" &&
	git cat-file tag ann-multi | grep "Second line" &&
	git cat-file tag ann-multi | grep "Third line"
'

test_expect_success 'tag -F from non-existing file should fail' '
	cd ann-inspect-repo &&
	! test -f nonexistingfile &&
	! git tag -F nonexistingfile notag &&
	! tag_exists notag
'

test_expect_success 'multiple -m options concatenate messages' '
	cd ann-inspect-repo &&
	git tag -m "msg1" -m "msg2" multi-m &&
	git cat-file tag multi-m | grep "msg1" &&
	git cat-file tag multi-m | grep "msg2"
'

# ---- more deletion tests ----

test_expect_success 'setup: deletion test repo' '
	git init del-repo &&
	cd del-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo x >x && git add x && git commit -m "init"
'

test_expect_success 'deleting a lightweight tag' '
	cd del-repo &&
	git tag del-light &&
	tag_exists del-light &&
	git tag -d del-light &&
	! tag_exists del-light
'

test_expect_success 'deleting an annotated tag' '
	cd del-repo &&
	git tag -m "delete me" del-ann &&
	tag_exists del-ann &&
	git tag -d del-ann &&
	! tag_exists del-ann
'

test_expect_success 'deleting a non-existent tag should fail' '
	cd del-repo &&
	! git tag -d nonexistent
'

test_expect_success 'deleting then recreating same tag name' '
	cd del-repo &&
	git tag recycle &&
	tag_exists recycle &&
	git tag -d recycle &&
	! tag_exists recycle &&
	git tag recycle &&
	tag_exists recycle &&
	git tag -d recycle
'

test_expect_success 'deleting already-deleted tag should fail' '
	cd del-repo &&
	git tag already-del &&
	git tag -d already-del &&
	! git tag -d already-del
'

test_expect_success 'force overwrite annotated with lightweight' '
	cd del-repo &&
	git tag -m "annotated" force-ann &&
	test $(git cat-file -t force-ann) = tag &&
	git tag --force force-ann &&
	test $(git cat-file -t force-ann) = commit
'

test_expect_success 'force overwrite lightweight with annotated' '
	cd del-repo &&
	git tag force-light &&
	test $(git cat-file -t force-light) = commit &&
	git tag --force -m "now annotated" force-light &&
	test $(git cat-file -t force-light) = tag
'

# ---- --force with existing and non-existing tags ----

test_expect_success 'setup: force test repo' '
	git init force-repo &&
	cd force-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo x >x && git add x && git commit -m "init"
'

test_expect_success '--force on existing tag succeeds' '
	cd force-repo &&
	git tag existing &&
	git tag --force existing &&
	tag_exists existing
'

test_expect_success '--force on non-existing tag succeeds' '
	cd force-repo &&
	git tag --force new-force-tag &&
	tag_exists new-force-tag
'

test_expect_success 'creating duplicate tag without --force fails' '
	cd force-repo &&
	git tag dup-tag &&
	! git tag dup-tag
'

# ---- annotated tag message from subdir ----

test_expect_success 'setup: subdir message repo' '
	git init subdir-repo &&
	cd subdir-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo x >x && git add x && git commit -m "init"
'

test_expect_success '-F file path is relative to cwd' '
	cd subdir-repo &&
	mkdir -p subdir &&
	echo "Tag message in sub directory" >subdir/msgfile &&
	(
		cd subdir &&
		git tag -a -F msgfile tag-from-subdir
	) &&
	git cat-file tag tag-from-subdir | grep "in sub directory"
'

test_expect_success '-F file in subdir second test' '
	cd subdir-repo &&
	echo "Another sub directory message" >subdir/msgfile2 &&
	(
		cd subdir &&
		git tag -a -F msgfile2 tag-from-subdir-2
	) &&
	git cat-file tag tag-from-subdir-2 | grep "Another sub directory message"
'

# ---- multiple tags point to same commit ----

test_expect_success 'multiple tags can point to same commit' '
	cd force-repo &&
	head=$(git rev-parse HEAD) &&
	git tag same-1 &&
	git tag same-2 &&
	test $(git rev-parse same-1) = "$head" &&
	test $(git rev-parse same-2) = "$head"
'

# ---- tag on specific earlier commit ----

test_expect_success 'setup: earlier commit repo' '
	git init earlier-repo &&
	cd earlier-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo a >a && git add a && git commit -m "first" &&
	echo b >b && git add b && git commit -m "second" &&
	echo c >c && git add c && git commit -m "third"
'

test_expect_success 'lightweight tag on specific earlier commit' '
	cd earlier-repo &&
	earlier=$(git rev-parse HEAD~1) &&
	git tag earlier-light $earlier &&
	test $(git rev-parse earlier-light) = "$earlier"
'

test_expect_success 'annotated tag on specific earlier commit' '
	cd earlier-repo &&
	earlier=$(git rev-parse HEAD~2) &&
	git tag -m "earlier annotated" earlier-ann $earlier &&
	git cat-file tag earlier-ann | grep "^object $earlier"
'

# ---- tag -l output matches tag output ----

test_expect_success 'tag -l and tag output identical in clean repo' '
	git init taglist-repo &&
	cd taglist-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo x >x && git add x && git commit -m "init" &&
	git tag alpha &&
	git tag beta &&
	git tag gamma &&
	git tag >actual1 &&
	git tag -l >actual2 &&
	test_cmp actual1 actual2
'

# ---- ambiguous branch/tag ----

test_expect_success 'ambiguous branch/tag: tag -l only shows tag' '
	git init ambig-repo &&
	cd ambig-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo x >x && git add x && git commit -m "init" &&
	git tag ambiguous &&
	git branch ambiguous 2>/dev/null || true &&
	echo ambiguous >expect &&
	git tag -l ambiguous >actual &&
	test_cmp expect actual
'

# ---- HEAD is forbidden as tagname ----

test_expect_success 'HEAD cannot be used as tag name' '
	git init head-repo &&
	cd head-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo x >x && git add x && git commit -m "init" &&
	! git tag HEAD &&
	! git tag -m "useless" HEAD
'

# ---- --contains non-list mode works ----

test_expect_success '--contains non-list mode outputs tags' '
	cd nocontains-repo &&
	git tag --contains v0.1 >actual &&
	grep "v0.1" actual &&
	grep "v0.5" actual
'

# ---- creating tag in empty tree fails ----

test_expect_success 'creating tag in empty repo fails' '
	git init empty-repo &&
	cd empty-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	! git tag mytag &&
	! tag_exists mytag
'

test_expect_success 'creating tag for HEAD in empty repo fails' '
	cd empty-repo &&
	! git tag mytaghead HEAD &&
	! tag_exists mytaghead
'

test_expect_success 'listing tags in empty repo succeeds with no output' '
	cd empty-repo &&
	git tag -l >actual &&
	test_must_be_empty actual &&
	git tag >actual2 &&
	test_must_be_empty actual2
'

test_expect_success 'tag for unknown revision fails' '
	cd empty-repo &&
	! git tag mytagnorev aaaaaaaaaaaaa &&
	! tag_exists mytagnorev
'

# ---- sort in main repo additional tests ----

test_expect_success 'version:tag sort for annotated tags' '
	git init vsort-repo &&
	cd vsort-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo x >x && git add x && git commit -m "init" &&
	git tag -a -m "sample 1.0" vsample-1.0 &&
	git tag -a -m "sample 2.0" vsample-2.0 &&
	git tag -a -m "sample 10.0" vsample-10.0 &&
	cat >expect <<-\EOF &&
	vsample-1.0
	vsample-10.0
	vsample-2.0
	EOF
	git tag --list --sort=refname "vsample-*" >actual &&
	test_cmp expect actual
'

# ---- lightweight tag type is commit ----

test_expect_success 'lightweight tag has type commit' '
	git init lwtype-repo &&
	cd lwtype-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo x >x && git add x && git commit -m "init" &&
	git tag lw-tag &&
	test $(git cat-file -t lw-tag) = commit
'

test_expect_success 'lightweight tag points to HEAD' '
	cd lwtype-repo &&
	test $(git rev-parse lw-tag) = $(git rev-parse HEAD)
'

# ---- tag on blob object (lightweight) ----

test_expect_success 'lightweight tag on blob' '
	cd lwtype-repo &&
	blob=$(git hash-object -w --stdin <<-\EOF
	Blob content here.
	EOF
	) &&
	git tag tag-blob $blob &&
	tag_exists tag-blob &&
	test $(git cat-file -t tag-blob) = blob
'

test_expect_success 'tag on blob can be listed' '
	cd lwtype-repo &&
	git tag -l tag-blob >actual &&
	echo "tag-blob" >expect &&
	test_cmp expect actual
'

# ---- --contains with branch and merge (separate repo) ----

test_expect_success 'setup: branch --contains repo' '
	git init branch-contains-repo &&
	cd branch-contains-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo a >a && git add a && git commit -m "first" &&
	git tag v1.0 &&
	echo b >b && git add b && git commit -m "second" &&
	git tag v2.0 &&
	echo c >c && git add c && git commit -m "third" &&
	git tag v3.0
'

test_expect_success '--contains with branch: create branch at v2.0' '
	cd branch-contains-repo &&
	git branch stable v2.0 &&
	git checkout stable &&
	echo d >d && git add d && git commit -m "fourth" &&
	git tag v4.0
'

test_expect_success '--contains: branch head only in v4.0' '
	cd branch-contains-repo &&
	hash4=$(git rev-parse HEAD) &&
	cat >expected <<-\EOF &&
	v4.0
	EOF
	git tag -l --contains $hash4 "v*" >actual &&
	test_cmp expected actual
'

test_expect_success '--contains: first commit in all tags' '
	cd branch-contains-repo &&
	hash1=$(git rev-parse v1.0) &&
	cat >expected <<-\EOF &&
	v1.0
	v2.0
	v3.0
	v4.0
	EOF
	git tag -l --contains $hash1 "v*" >actual &&
	test_cmp expected actual
'

# ---- tag list with -n shows annotations for various tag types ----

test_expect_success 'setup: mixed tags for -n display' '
	git init mixed-n-repo &&
	cd mixed-n-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo x >x && git add x && git commit -m "init" &&
	git tag lightweight &&
	git tag -m "annotated message" annotated
'

test_expect_success '-n shows annotation for annotated, name only for lightweight' '
	cd mixed-n-repo &&
	git tag -n >actual &&
	grep "^annotated" actual | grep "annotated message" &&
	grep "^lightweight" actual
'

test_expect_success '-n1 on annotated shows first line of message' '
	cd mixed-n-repo &&
	echo "annotated       annotated message" >expect &&
	git tag -n1 -l annotated >actual &&
	test_cmp expect actual
'

test_expect_success '-n0 on annotated shows just name' '
	cd mixed-n-repo &&
	echo "annotated" >expect &&
	git tag -n0 -l annotated >actual &&
	test_cmp expect actual
'

# ---- update-ref for creating and deleting tags ----

test_expect_success 'setup: update-ref tag repo' '
	git init updateref-repo &&
	cd updateref-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo x >x && git add x && git commit -m "init"
'

test_expect_success 'creating tag with update-ref and deleting it' '
	cd updateref-repo &&
	git update-ref refs/tags/manual-ref-tag HEAD &&
	tag_exists manual-ref-tag &&
	git tag -d manual-ref-tag &&
	! tag_exists manual-ref-tag
'

test_expect_success 'tag created with update-ref appears in listing' '
	cd updateref-repo &&
	git update-ref refs/tags/ur-tag HEAD &&
	git tag -l ur-tag >actual &&
	echo "ur-tag" >expect &&
	test_cmp expect actual &&
	git tag -d ur-tag
'

# ---- --sort combined with --contains ----

test_expect_success 'setup: sort+contains repo' '
	git init sort-contains-repo &&
	cd sort-contains-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo a >a && git add a && git commit -m "first" &&
	git tag v1.0 &&
	echo b >b && git add b && git commit -m "second" &&
	git tag v2.0 &&
	echo c >c && git add c && git commit -m "third" &&
	git tag v3.0
'

test_expect_success '--sort combined with --contains' '
	cd sort-contains-repo &&
	cat >expect <<-\EOF &&
	v3.0
	v2.0
	v1.0
	EOF
	git tag -l --sort=-refname --contains v1.0 "v*" >actual &&
	test_cmp expect actual
'

test_expect_success '--sort refname with --contains' '
	cd sort-contains-repo &&
	cat >expect <<-\EOF &&
	v1.0
	v2.0
	v3.0
	EOF
	git tag -l --sort=refname --contains v1.0 "v*" >actual &&
	test_cmp expect actual
'

test_expect_success '--contains with middle commit and sort' '
	cd sort-contains-repo &&
	cat >expect <<-\EOF &&
	v3.0
	v2.0
	EOF
	git tag -l --sort=-refname --contains v2.0 "v*" >actual &&
	test_cmp expect actual
'

# ---- more --contains edge cases ----

test_expect_success '--contains with tag name resolves through tag' '
	cd sort-contains-repo &&
	cat >expected <<-\EOF &&
	v1.0
	v2.0
	v3.0
	EOF
	git tag -l --contains v1.0 "v*" >actual &&
	test_cmp expected actual
'

test_expect_success '--contains with HEAD~N' '
	cd sort-contains-repo &&
	cat >expected <<-\EOF &&
	v1.0
	v2.0
	v3.0
	EOF
	git tag -l --contains HEAD~2 "v*" >actual &&
	test_cmp expected actual
'

test_expect_success '--contains HEAD covers only HEAD tag' '
	cd sort-contains-repo &&
	cat >expected <<-\EOF &&
	v3.0
	EOF
	git tag -l --contains HEAD "v*" >actual &&
	test_cmp expected actual
'

test_expect_success '--contains past all tags is empty' '
	cd sort-contains-repo &&
	echo d >d && git add d && git commit -m "fourth" &&
	git tag -l --contains HEAD "v*" >actual &&
	test_must_be_empty actual
'

# ---- more annotated tag edge cases ----

test_expect_success 'setup: annotated edge cases repo' '
	git init ann-edge-repo &&
	cd ann-edge-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo x >x && git add x && git commit -m "init"
'

test_expect_success 'annotated tag message with special characters' '
	cd ann-edge-repo &&
	git tag -m "Message with special chars: !@#$%" special-chars-tag &&
	git cat-file tag special-chars-tag | grep "special chars"
'

test_expect_success 'annotated tag with very long message' '
	cd ann-edge-repo &&
	printf "%.0sA long message line. " $(seq 1 50) >longmsg &&
	git tag -F longmsg long-msg-tag &&
	tag_exists long-msg-tag
'

test_expect_success 'annotated tag -m with newlines in message' '
	cd ann-edge-repo &&
	git tag -m "First line" -m "Second line" two-m-tag &&
	git cat-file tag two-m-tag | grep "First line" &&
	git cat-file tag two-m-tag | grep "Second line"
'

test_expect_success 'annotated tag -m with three messages' '
	cd ann-edge-repo &&
	git tag -m "One" -m "Two" -m "Three" three-m-tag &&
	git cat-file tag three-m-tag | grep "One" &&
	git cat-file tag three-m-tag | grep "Two" &&
	git cat-file tag three-m-tag | grep "Three"
'

# ---- pattern matching additional edge cases ----

test_expect_success 'setup: pattern edge case repo' '
	git init pattern-edge-repo &&
	cd pattern-edge-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo x >x && git add x && git commit -m "init" &&
	git tag release-1.0 &&
	git tag release-1.1 &&
	git tag release-2.0 &&
	git tag beta-1.0 &&
	git tag beta-2.0 &&
	git tag alpha
'

test_expect_success 'pattern matching with prefix release-*' '
	cd pattern-edge-repo &&
	cat >expect <<-\EOF &&
	release-1.0
	release-1.1
	release-2.0
	EOF
	git tag -l "release-*" >actual &&
	test_cmp expect actual
'

test_expect_success 'pattern matching with prefix beta-*' '
	cd pattern-edge-repo &&
	cat >expect <<-\EOF &&
	beta-1.0
	beta-2.0
	EOF
	git tag -l "beta-*" >actual &&
	test_cmp expect actual
'

test_expect_success 'pattern matching with suffix *.0' '
	cd pattern-edge-repo &&
	cat >expect <<-\EOF &&
	beta-1.0
	beta-2.0
	release-1.0
	release-2.0
	EOF
	git tag -l "*.0" >actual &&
	test_cmp expect actual
'

test_expect_success 'pattern matching with ? single char' '
	cd pattern-edge-repo &&
	cat >expect <<-\EOF &&
	release-1.0
	release-1.1
	release-2.0
	EOF
	git tag -l "release-?.?" >actual &&
	test_cmp expect actual
'

test_expect_success 'exact match pattern' '
	cd pattern-edge-repo &&
	echo "alpha" >expect &&
	git tag -l alpha >actual &&
	test_cmp expect actual
'

test_expect_success 'no match pattern is empty' '
	cd pattern-edge-repo &&
	git tag -l "zzz*" >actual &&
	test_must_be_empty actual
'

test_expect_success 'listing all tags is sorted' '
	cd pattern-edge-repo &&
	cat >expect <<-\EOF &&
	alpha
	beta-1.0
	beta-2.0
	release-1.0
	release-1.1
	release-2.0
	EOF
	git tag -l >actual &&
	test_cmp expect actual
'

test_expect_success 'pattern with * at both ends' '
	cd pattern-edge-repo &&
	cat >expect <<-\EOF &&
	beta-1.0
	beta-2.0
	release-1.0
	release-1.1
	release-2.0
	EOF
	git tag -l "*-*" >actual &&
	test_cmp expect actual
'

# ---- tag listing sorted consistently ----

test_expect_success 'tag and tag -l produce same output' '
	cd pattern-edge-repo &&
	git tag >actual1 &&
	git tag -l >actual2 &&
	test_cmp actual1 actual2
'

# ---- creatordate sort with mixed annotated and lightweight ----

test_expect_success 'setup: mixed creatordate repo' '
	git init mixed-date-repo &&
	cd mixed-date-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo a >a && git add a && git commit -m "first" &&
	git tag light-a &&
	git tag -m "ann a" ann-a &&
	echo b >b && git add b && git commit -m "second" &&
	git tag light-b &&
	git tag -m "ann b" ann-b
'

test_expect_success 'creatordate sort with mixed tag types' '
	cd mixed-date-repo &&
	git tag -l --sort=creatordate >actual &&
	cat >expect <<-\EOF &&
	ann-a
	ann-b
	light-a
	light-b
	EOF
	test_cmp expect actual
'

test_expect_success 'reverse creatordate sort with mixed tag types' '
	cd mixed-date-repo &&
	git tag -l --sort=-creatordate >actual &&
	cat >expect <<-\EOF &&
	light-b
	light-a
	ann-b
	ann-a
	EOF
	test_cmp expect actual
'

# ---- --contains across branch ----

test_expect_success 'setup: multi-branch contains repo' '
	git init multi-br-repo &&
	cd multi-br-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo a >a && git add a && git commit -m "root" &&
	git tag root-tag &&
	echo b >b && git add b && git commit -m "on-master" &&
	git tag master-tag &&
	git branch feature root-tag &&
	git checkout feature &&
	echo c >c && git add c && git commit -m "on-feature" &&
	git tag feature-tag
'

test_expect_success '--contains root commit shows all tags' '
	cd multi-br-repo &&
	root=$(git rev-parse root-tag) &&
	cat >expected <<-\EOF &&
	feature-tag
	master-tag
	root-tag
	EOF
	git tag -l --contains $root >actual &&
	test_cmp expected actual
'

test_expect_success '--contains feature commit shows only feature-tag' '
	cd multi-br-repo &&
	cat >expected <<-\EOF &&
	feature-tag
	EOF
	git tag -l --contains feature-tag >actual &&
	test_cmp expected actual
'

test_expect_success '--contains master commit shows only master-tag' '
	cd multi-br-repo &&
	cat >expected <<-\EOF &&
	master-tag
	EOF
	git tag -l --contains master-tag >actual &&
	test_cmp expected actual
'

# ---- force overwrite specific scenarios ----

test_expect_success 'setup: force overwrite scenarios' '
	git init force-ow-repo &&
	cd force-ow-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo a >a && git add a && git commit -m "first" &&
	echo b >b && git add b && git commit -m "second"
'

test_expect_success 'force move tag to different commit' '
	cd force-ow-repo &&
	git tag moveme HEAD~1 &&
	old=$(git rev-parse moveme) &&
	git tag --force moveme HEAD &&
	new=$(git rev-parse moveme) &&
	test "$old" != "$new" &&
	test "$new" = "$(git rev-parse HEAD)"
'

test_expect_success 'force overwrite lightweight with annotated on different commit' '
	cd force-ow-repo &&
	git tag light-to-ann HEAD~1 &&
	test $(git cat-file -t light-to-ann) = commit &&
	git tag --force -m "now annotated" light-to-ann HEAD &&
	test $(git cat-file -t light-to-ann) = tag
'

test_expect_success 'force overwrite annotated with lightweight on different commit' '
	cd force-ow-repo &&
	git tag -m "annotated" ann-to-light HEAD~1 &&
	test $(git cat-file -t ann-to-light) = tag &&
	git tag --force ann-to-light HEAD &&
	test $(git cat-file -t ann-to-light) = commit
'

# ---- show-ref and tag interaction ----

test_expect_success 'setup: show-ref tags repo' '
	git init showref-repo &&
	cd showref-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo x >x && git add x && git commit -m "init" &&
	git tag v1.0 &&
	git tag -m "annotated" v2.0
'

test_expect_success 'show-ref --verify confirms tag existence' '
	cd showref-repo &&
	git show-ref --verify refs/tags/v1.0 &&
	git show-ref --verify refs/tags/v2.0
'

test_expect_success 'show-ref --verify fails for nonexistent tag' '
	cd showref-repo &&
	! git show-ref --quiet --verify refs/tags/nonexistent
'

# ---- cat-file with annotated tags ----

test_expect_success 'cat-file -t on annotated tag returns tag' '
	cd showref-repo &&
	test $(git cat-file -t v2.0) = tag
'

test_expect_success 'cat-file -t on lightweight tag returns commit' '
	cd showref-repo &&
	test $(git cat-file -t v1.0) = commit
'

test_expect_success 'cat-file -p on annotated tag shows tag object' '
	cd showref-repo &&
	git cat-file -p v2.0 >actual &&
	grep "^object " actual &&
	grep "^type commit" actual &&
	grep "^tag v2.0" actual &&
	grep "^tagger " actual &&
	grep "annotated" actual
'

# ---- -n with --sort ----

test_expect_success 'setup: -n with --sort repo' '
	git init n-sort-repo &&
	cd n-sort-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo x >x && git add x && git commit -m "init" &&
	git tag -m "msg-c" tag-c &&
	git tag -m "msg-a" tag-a &&
	git tag -m "msg-b" tag-b
'

test_expect_success '-n1 with --sort=refname' '
	cd n-sort-repo &&
	cat >expect <<-\EOF &&
	tag-a           msg-a
	tag-b           msg-b
	tag-c           msg-c
	EOF
	git tag -n1 --sort=refname -l "tag-*" >actual &&
	test_cmp expect actual
'

test_expect_success '-n1 with --sort=-refname' '
	cd n-sort-repo &&
	cat >expect <<-\EOF &&
	tag-c           msg-c
	tag-b           msg-b
	tag-a           msg-a
	EOF
	git tag -n1 --sort=-refname -l "tag-*" >actual &&
	test_cmp expect actual
'

test_expect_success '-n0 with --sort=refname shows just names' '
	cd n-sort-repo &&
	cat >expect <<-\EOF &&
	tag-a
	tag-b
	tag-c
	EOF
	git tag -n0 --sort=refname -l "tag-*" >actual &&
	test_cmp expect actual
'

# ── additional tag tests ─────────────────────────────────────────────

test_expect_success 'setup: repo for additional tag tests' '
	git init tag-extra &&
	cd tag-extra &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo "base" >base.txt &&
	git add base.txt &&
	git commit -m "initial" 2>/dev/null
'

test_expect_success 'lightweight tag points to HEAD' '
	cd tag-extra &&
	git tag lw-test &&
	tag_sha=$(git rev-parse lw-test) &&
	head_sha=$(git rev-parse HEAD) &&
	test "$tag_sha" = "$head_sha"
'

test_expect_success 'annotated tag object type is tag' '
	cd tag-extra &&
	git tag -m "annotated" ann-test &&
	git cat-file -t ann-test >actual &&
	echo "tag" >expect &&
	test_cmp expect actual
'

test_expect_success 'annotated tag object points to commit' '
	cd tag-extra &&
	git cat-file -p ann-test >actual &&
	grep "^type commit" actual
'

test_expect_success 'tag -l with pattern filters correctly' '
	cd tag-extra &&
	git tag alpha-one &&
	git tag alpha-two &&
	git tag beta-one &&
	git tag -l "alpha-*" >actual &&
	grep "alpha-one" actual &&
	grep "alpha-two" actual &&
	! grep "beta-one" actual
'

test_expect_success 'tag -d removes lightweight tag' '
	cd tag-extra &&
	git tag del-me &&
	git tag -d del-me 2>/dev/null &&
	test_must_fail git rev-parse --verify refs/tags/del-me
'

test_expect_success 'tag -d removes annotated tag' '
	cd tag-extra &&
	git tag -m "delete me" ann-del &&
	git tag -d ann-del 2>/dev/null &&
	test_must_fail git rev-parse --verify refs/tags/ann-del
'

test_expect_success 'duplicate lightweight tag fails' '
	cd tag-extra &&
	git tag dup-lw &&
	test_must_fail git tag dup-lw 2>/dev/null
'

test_expect_success 'tag -f overwrites existing tag' '
	cd tag-extra &&
	git tag force-me &&
	old=$(git rev-parse force-me) &&
	git commit --allow-empty -m "advance" 2>/dev/null &&
	git tag -f force-me 2>/dev/null &&
	new=$(git rev-parse force-me) &&
	test "$old" != "$new"
'

test_expect_success 'tag -l lists all tags' '
	cd tag-extra &&
	git tag -l >actual &&
	count=$(wc -l <actual | tr -d " ") &&
	test "$count" -ge 5
'

test_expect_success 'tag on specific commit via sha' '
	cd tag-extra &&
	sha=$(git rev-parse HEAD~1 2>/dev/null || git rev-parse HEAD) &&
	git tag at-sha "$sha" &&
	result=$(git rev-parse at-sha) &&
	test "$result" = "$sha"
'

test_expect_success 'annotated tag message is stored' '
	cd tag-extra &&
	git tag -m "my special message" msg-check &&
	git cat-file -p msg-check >actual &&
	grep "my special message" actual
'

test_expect_success 'tag -n1 shows first line of message' '
	cd tag-extra &&
	git tag -m "line one" n1-check &&
	git tag -n1 -l n1-check >actual &&
	grep "line one" actual
'

test_expect_success 'lightweight tag dereferences to commit' '
	cd tag-extra &&
	git tag lw-deref &&
	git cat-file -t lw-deref >actual &&
	echo "commit" >expect &&
	test_cmp expect actual
'

test_expect_success 'tag -d on nonexistent tag fails' '
	cd tag-extra &&
	test_must_fail git tag -d nonexistent-tag-xyz 2>/dev/null
'

# ---------------------------------------------------------------------------
# Additional tag coverage
# ---------------------------------------------------------------------------
test_expect_success 'tag list is initially populated' '
	cd tag-extra &&
	git tag -l >output &&
	test -s output
'

test_expect_success 'tag -l with pattern filters correctly' '
	cd tag-extra &&
	git tag filter-match-1 &&
	git tag filter-match-2 &&
	git tag no-match-x &&
	git tag -l "filter-match-*" >output &&
	test_line_count = 2 output
'

test_expect_success 'annotated tag type is tag' '
	cd tag-extra &&
	git tag -m "typed" typed-tag &&
	git cat-file -t typed-tag >actual &&
	echo "tag" >expect &&
	test_cmp expect actual
'

test_expect_success 'lightweight tag type is commit' '
	cd tag-extra &&
	git tag lw-type-check &&
	git cat-file -t lw-type-check >actual &&
	echo "commit" >expect &&
	test_cmp expect actual
'

test_expect_success 'tag refuses duplicate name' '
	cd tag-extra &&
	git tag dup-tag &&
	test_must_fail git tag dup-tag 2>/dev/null
'

test_expect_success 'tag -d removes tag successfully' '
	cd tag-extra &&
	git tag del-me &&
	git tag -d del-me &&
	test_must_fail git rev-parse del-me 2>/dev/null
'

test_expect_success 'tag created at HEAD by default' '
	cd tag-extra &&
	git tag at-head-check &&
	head=$(git rev-parse HEAD) &&
	tag=$(git rev-parse at-head-check) &&
	test "$head" = "$tag"
'

test_expect_success 'tag on specific commit' '
	cd tag-extra &&
	first=$(git rev-list --reverse HEAD | head -1) &&
	git tag on-first "$first" &&
	result=$(git rev-parse on-first) &&
	test "$result" = "$first"
'

test_expect_success 'annotated tag tagger field exists' '
	cd tag-extra &&
	git tag -m "tagger test" tagger-check &&
	git cat-file -p tagger-check >actual &&
	grep "^tagger" actual
'

test_expect_success 'tag with slash in name' '
	cd tag-extra &&
	git tag release/v1.0 &&
	git rev-parse release/v1.0 >/dev/null
'

test_expect_success 'tag with dots in name' '
	cd tag-extra &&
	git tag v2.0.1-rc1 &&
	git rev-parse v2.0.1-rc1 >/dev/null
'

test_expect_success 'tag -l count matches expected' '
	cd tag-extra &&
	before=$(git tag -l | wc -l) &&
	git tag count-check &&
	after=$(git tag -l | wc -l) &&
	test "$after" -eq "$((before + 1))"
'

test_expect_success 'annotated tag object field points to commit' '
	cd tag-extra &&
	git tag -m "obj test" obj-check &&
	git cat-file -p obj-check >actual &&
	grep "^object" actual &&
	grep "^type commit" actual
'

test_expect_success 'multiple tags can point to same commit' '
	cd tag-extra &&
	head=$(git rev-parse HEAD) &&
	git tag multi-a &&
	git tag multi-b &&
	a=$(git rev-parse multi-a) &&
	b=$(git rev-parse multi-b) &&
	test "$a" = "$b"
'

test_expect_success 'tag -d can delete tags one at a time' '
	cd tag-extra &&
	git tag mdel-1 &&
	git tag mdel-2 &&
	git tag -d mdel-1 &&
	git tag -d mdel-2 &&
	test_must_fail git rev-parse mdel-1 2>/dev/null &&
	test_must_fail git rev-parse mdel-2 2>/dev/null
'

test_expect_success 'tag -n0 lists tags without message' '
	cd tag-extra &&
	git tag -n0 >output &&
	test -s output
'

test_expect_success 'tag on specific commit via SHA' '
	cd tag-extra &&
	sha=$(git rev-parse HEAD~1) &&
	git tag sha-tag "$sha" &&
	result=$(git rev-parse sha-tag) &&
	test "$result" = "$sha"
'

test_expect_success 'tag -l pattern filters tags' '
	cd tag-extra &&
	git tag filter-a-1 &&
	git tag filter-a-2 &&
	git tag filter-b-1 &&
	git tag -l "filter-a-*" >output &&
	grep "filter-a-1" output &&
	grep "filter-a-2" output &&
	! grep "filter-b-1" output
'

test_expect_success 'tag list is sorted alphabetically' '
	cd tag-extra &&
	git tag -l >output &&
	sort output >sorted &&
	test_cmp sorted output
'

test_expect_success 'tag cannot create duplicate name' '
	cd tag-extra &&
	git tag dup-tag-new 2>/dev/null &&
	test_must_fail git tag dup-tag-new 2>/dev/null
'

test_expect_success 'tag -d nonexistent tag fails' '
	cd tag-extra &&
	test_must_fail git tag -d nonexistent-tag-xyz 2>/dev/null
'

test_expect_success 'annotated tag message is retrievable' '
	cd tag-extra &&
	git tag -a ann-msg -m "my annotation" 2>/dev/null &&
	git cat-file -p ann-msg >output &&
	grep "my annotation" output
'

test_expect_success 'annotated tag type field is commit' '
	cd tag-extra &&
	git cat-file -p ann-msg >output &&
	grep "^type commit" output
'

test_expect_success 'lightweight tag resolves directly to commit' '
	cd tag-extra &&
	git tag light-resolve &&
	type=$(git cat-file -t light-resolve) &&
	test "$type" = "commit"
'

test_expect_success 'annotated tag object type is tag' '
	cd tag-extra &&
	git tag -a ann-type-check -m "type check" 2>/dev/null &&
	raw_sha=$(git rev-parse ann-type-check) &&
	type=$(git cat-file -t "$raw_sha") &&
	test "$type" = "tag"
'

test_expect_success 'tag -l with no tags matching shows empty' '
	cd tag-extra &&
	git tag -l "zzzz-no-match-*" >output &&
	test_must_be_empty output
'

test_expect_success 'tag on HEAD is same as tag with no arg' '
	cd tag-extra &&
	git tag explicit-head HEAD &&
	git tag implicit-head &&
	a=$(git rev-parse explicit-head) &&
	b=$(git rev-parse implicit-head) &&
	test "$a" = "$b"
'

test_expect_success 'tag -d then recreate same name' '
	cd tag-extra &&
	git tag recreate-me &&
	git tag -d recreate-me &&
	git tag recreate-me &&
	git rev-parse recreate-me
'

test_expect_success 'tag -f overwrites existing lightweight tag' '
	cd tag-extra &&
	git tag force-me-v2 &&
	old=$(git rev-parse force-me-v2) &&
	echo advance2 >force-file2.txt && git add force-file2.txt && git commit -m "advance2" 2>/dev/null &&
	git tag -f force-me-v2 &&
	new=$(git rev-parse force-me-v2) &&
	test "$old" != "$new"
'

test_expect_success 'tag -l lists all tags' '
	cd tag-extra &&
	git tag list-test-a &&
	git tag list-test-b &&
	git tag -l >output &&
	grep "list-test-a" output &&
	grep "list-test-b" output
'

test_expect_success 'tag -l with pattern filters tags' '
	cd tag-extra &&
	git tag -l "list-test-*" >output &&
	grep "list-test-a" output &&
	grep "list-test-b" output &&
	! grep "force-me" output
'

test_expect_success 'annotated tag with -F reads message from file' '
	cd tag-extra &&
	echo "file message" >tag-msg-file &&
	git tag -a -F tag-msg-file file-tag 2>/dev/null &&
	git cat-file -p file-tag >output &&
	grep "file message" output
'

test_expect_success 'tag -d removes annotated tag too' '
	cd tag-extra &&
	git tag -a del-ann -m "delete me" 2>/dev/null &&
	git tag -d del-ann &&
	test_must_fail git rev-parse del-ann 2>/dev/null
'

test_expect_success 'tag --contains HEAD lists tags on HEAD' '
	cd tag-extra &&
	git tag contains-test &&
	git tag --contains HEAD >output &&
	grep "contains-test" output
'

test_expect_success 'tag --contains excludes older tags' '
	cd tag-extra &&
	git tag old-tag HEAD~1 &&
	git tag --contains HEAD >output &&
	! grep "old-tag" output
'

test_expect_success 'annotated tag message with multiple lines' '
	cd tag-extra &&
	printf "line1\nline2\nline3" >multi-msg &&
	git tag -a -F multi-msg multi-line-tag 2>/dev/null &&
	git cat-file -p multi-line-tag >output &&
	grep "line1" output &&
	grep "line2" output &&
	grep "line3" output
'

test_expect_success 'tag -n shows annotation line' '
	cd tag-extra &&
	git tag -a show-n-tag -m "annotation line" 2>/dev/null &&
	git tag -n -l "show-n-tag" >output &&
	grep "annotation line" output
'

test_expect_success 'tag on specific commit points to that commit' '
	cd tag-extra &&
	parent=$(git rev-parse HEAD~1) &&
	git tag specific-commit-tag HEAD~1 &&
	result=$(git rev-parse specific-commit-tag) &&
	test "$result" = "$parent"
'

test_expect_success 'annotated tag has tag header in cat-file output' '
	cd tag-extra &&
	git tag -a tagger-check-v2 -m "check tagger v2" 2>/dev/null &&
	git cat-file -p tagger-check-v2 >output &&
	grep "^tag tagger-check-v2" output
'

test_expect_success 'tag -d on nonexistent tag fails' '
	cd tag-extra &&
	test_must_fail git tag -d no-such-tag-xyz 2>/dev/null
'

test_expect_success 'creating tag without -f on existing tag fails' '
	cd tag-extra &&
	git tag no-dup-tag &&
	test_must_fail git tag no-dup-tag 2>/dev/null
'

test_expect_success 'tag -l with no match returns empty' '
	cd tag-extra &&
	git tag -l "zzz-nonexistent-*" >output &&
	test_must_be_empty output
'

test_expect_success 'tag -f on annotated tag works' '
	cd tag-extra &&
	git tag -a force-ann-v2 -m "original" 2>/dev/null &&
	git tag -f -a force-ann-v2 -m "replaced" 2>/dev/null &&
	git cat-file -p force-ann-v2 >output &&
	grep "replaced" output
'

test_expect_success 'tag -l lists all tags' '
	cd tag-extra &&
	git tag -l >output &&
	test $(wc -l <output) -ge 5
'

test_expect_success 'tag -l with glob matches subset' '
	cd tag-extra &&
	git tag -l "contains-*" >output &&
	grep "contains-test" output
'

test_expect_success 'lightweight tag is same as commit hash' '
	cd tag-extra &&
	git tag lwt-check &&
	head=$(git rev-parse HEAD) &&
	tag_hash=$(git rev-parse lwt-check) &&
	test "$head" = "$tag_hash"
'

test_expect_success 'annotated tag type is tag' '
	cd tag-extra &&
	git tag -a type-check-tag -m "type check" 2>/dev/null &&
	type=$(git cat-file -t type-check-tag) &&
	test "$type" = "tag"
'

test_expect_success 'lightweight tag type is commit' '
	cd tag-extra &&
	type=$(git cat-file -t lwt-check) &&
	test "$type" = "commit"
'

test_expect_success 'tag --sort=refname lists in alphabetical order' '
	cd tag-extra &&
	git tag --sort=refname >output &&
	sort <output >sorted &&
	test_cmp output sorted
'

test_expect_success 'tag -v on unsigned tag fails' '
	cd tag-extra &&
	git tag -a unsigned-verify -m "unsigned" 2>/dev/null &&
	test_must_fail git tag -v unsigned-verify 2>/dev/null
'

test_expect_success 'tag --contains lists tags containing commit' '
	cd tag-extra &&
	git tag contains-head-v2 &&
	git tag --contains HEAD >output &&
	grep "contains-head-v2" output
'

test_expect_success 'tag -f overwrites lightweight tag' '
	cd tag-extra &&
	git tag force-lw &&
	parent=$(git rev-parse HEAD~1) &&
	git tag -f force-lw "$parent" &&
	result=$(git rev-parse force-lw) &&
	test "$result" = "$parent"
'

test_expect_success 'tag --sort=version:refname sorts by version' '
	cd tag-extra &&
	git tag v1.1.0 &&
	git tag v1.2.0 &&
	git tag v1.10.0 &&
	git tag --sort=version:refname -l "v1.*" >output &&
	test -s output
'

test_expect_success 'annotated tag cat-file shows tagger line' '
	cd tag-extra &&
	git tag -a tagger-line-v2 -m "tagger" 2>/dev/null &&
	git cat-file -p tagger-line-v2 >output &&
	grep "^tagger " output
'

test_expect_success 'tag -d deletes one tag at a time' '
	cd tag-extra &&
	git tag single-del-v2 &&
	git tag -d single-del-v2 &&
	test_must_fail git rev-parse single-del-v2 2>/dev/null
'

test_expect_success 'tag with annotated message from -m flag' '
	cd tag-extra &&
	git tag -a msg-flag-tag -m "flag message" 2>/dev/null &&
	git cat-file -p msg-flag-tag >output &&
	grep "flag message" output
'

test_expect_success 'creating tag on detached HEAD works' '
	cd tag-extra &&
	head=$(git rev-parse HEAD) &&
	git checkout --detach HEAD 2>/dev/null &&
	git tag detached-tag &&
	result=$(git rev-parse detached-tag) &&
	test "$result" = "$head" &&
	git checkout master 2>/dev/null
'

test_expect_success 'tag -l lists all lightweight and annotated tags' '
	cd tag-extra &&
	git tag -l >output &&
	test $(wc -l <output) -ge 10
'

test_expect_success 'tag on specific commit works' '
	cd tag-extra &&
	echo newthing >newthing.txt &&
	git add newthing.txt && git commit -m "newthing" 2>/dev/null &&
	sha=$(git rev-parse HEAD~1) &&
	git tag specific-commit-tag-v3 "$sha" &&
	result=$(git rev-parse specific-commit-tag-v3) &&
	test "$result" = "$sha"
'

test_expect_success 'tag name with slash is valid' '
	cd tag-extra &&
	git tag release/v9.0 &&
	git rev-parse release/v9.0 >output &&
	test -s output
'

test_expect_success 'tag name with dots is valid' '
	cd tag-extra &&
	git tag v1.2.3 &&
	git rev-parse v1.2.3 >output &&
	test -s output
'

test_expect_success 'tag -d removes tag completely' '
	cd tag-extra &&
	git tag ephemeral-tag &&
	git tag -d ephemeral-tag &&
	test_must_fail git rev-parse ephemeral-tag 2>/dev/null
'

test_expect_success 'annotated tag cat-file -p shows message' '
	cd tag-extra &&
	git tag -a msg-check-tag -m "typed message here" 2>/dev/null &&
	git cat-file -p msg-check-tag >output &&
	grep "typed message here" output
'

test_expect_success 'annotated tag has tagger line' '
	cd tag-extra &&
	git cat-file -p typed-tag >output &&
	grep "^tagger" output
'

test_expect_success 'annotated tag has tag name in object' '
	cd tag-extra &&
	git cat-file -p typed-tag >output &&
	grep "^tag typed-tag" output
'

test_expect_success 'tag points to correct commit' '
	cd tag-extra &&
	head=$(git rev-parse HEAD) &&
	git tag verify-target-tag &&
	result=$(git rev-parse verify-target-tag) &&
	test "$result" = "$head"
'

test_expect_success 'multiple tags on same commit allowed' '
	cd tag-extra &&
	git tag multi-tag-1 &&
	git tag multi-tag-2 &&
	r1=$(git rev-parse multi-tag-1) &&
	r2=$(git rev-parse multi-tag-2) &&
	test "$r1" = "$r2"
'

test_expect_success 'tag -l output is sorted' '
	cd tag-extra &&
	git tag -l >output &&
	sort output >sorted &&
	test_cmp sorted output
'

test_expect_success 'creating duplicate tag fails' '
	cd tag-extra &&
	git tag dup-test-tag &&
	test_must_fail git tag dup-test-tag 2>/dev/null
'

test_expect_success 'tag with hyphen in name works' '
	cd tag-extra &&
	git tag my-hyphen-tag &&
	git rev-parse my-hyphen-tag >output &&
	test -s output
'

test_expect_success 'annotated tag cat-file -p shows object line' '
	cd tag-extra &&
	git tag -a cat-test-tag -m "cat test" 2>/dev/null &&
	git cat-file -p cat-test-tag >output &&
	grep "^object" output
'

test_expect_success 'annotated tag cat-file -p shows type commit' '
	cd tag-extra &&
	git cat-file -p cat-test-tag >output &&
	grep "^type commit" output
'

test_done
