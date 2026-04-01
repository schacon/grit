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

test_done
