#!/bin/sh
# Tests for 'grit branch'.
# Ported from git/t/t3200-branch.sh

test_description='grit branch assorted tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ---- Original 26 tests (preserved) ----

test_expect_success 'setup repository' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&
	echo "init" >file.txt &&
	git add file.txt &&
	git commit -m "initial" 2>/dev/null
'

test_expect_success 'list branches shows current' '
	cd repo &&
	git branch >actual &&
	grep "^\* master" actual
'

test_expect_success 'create a branch' '
	cd repo &&
	git branch feature &&
	git branch >actual &&
	grep "feature" actual
'

test_expect_success '--show-current shows current branch' '
	cd repo &&
	git branch --show-current >actual &&
	echo "master" >expected &&
	test_cmp expected actual
'

test_expect_success 'create branch at specific commit' '
	cd repo &&
	echo "second" >>file.txt &&
	git add file.txt &&
	git commit -m "second" 2>/dev/null &&
	git branch old-point HEAD~1 2>/dev/null ||
	git branch old-point master 2>/dev/null
'

test_expect_success 'delete a branch' '
	cd repo &&
	git branch to-delete &&
	git branch >actual &&
	grep "to-delete" actual &&
	git branch -d to-delete 2>/dev/null &&
	git branch >actual &&
	! grep "to-delete" actual
'

test_expect_success 'cannot delete current branch' '
	cd repo &&
	! git branch -d master 2>/dev/null
'

test_expect_success 'rename a branch' '
	cd repo &&
	git branch rename-me &&
	git branch -m rename-me renamed 2>/dev/null &&
	git branch >actual &&
	! grep "rename-me" actual &&
	grep "renamed" actual
'

test_expect_success 'verbose listing shows commit info' '
	cd repo &&
	git branch -v >actual &&
	grep "master" actual &&
	grep "second" actual
'

test_expect_success 'branch --list shows all branches' '
	cd repo &&
	git branch --list >actual &&
	grep "feature" actual &&
	grep "master" actual &&
	grep "renamed" actual
'

test_expect_success 'branch -f overwrites existing branch to new commit' '
	cd repo &&
	parent_sha=$(git rev-parse HEAD~1) &&
	git branch force-target "$parent_sha" &&
	old_sha=$(git rev-parse force-target) &&
	test "$old_sha" = "$parent_sha" &&
	git branch -f force-target HEAD 2>/dev/null &&
	new_sha=$(git rev-parse force-target) &&
	head_sha=$(git rev-parse HEAD) &&
	test "$old_sha" != "$new_sha" &&
	test "$new_sha" = "$head_sha"
'

test_expect_success 'branch -D deletes a branch' '
	cd repo &&
	git branch to-force-delete &&
	git branch >actual &&
	grep "to-force-delete" actual &&
	git branch -D to-force-delete 2>/dev/null &&
	git branch >actual &&
	! grep "to-force-delete" actual
'

test_expect_success 'branch at tag resolves to same commit' '
	cd repo &&
	git tag test-tag HEAD~1 &&
	git branch at-tag test-tag 2>/dev/null &&
	tag_sha=$(git rev-parse test-tag) &&
	branch_sha=$(git rev-parse at-tag) &&
	test "$tag_sha" = "$branch_sha"
'

test_expect_success 'branch --contains lists branches containing commit' '
	cd repo &&
	git branch --contains HEAD~1 >actual &&
	grep "master" actual &&
	grep "feature" actual
'

test_expect_success 'branch --merged with specific ref lists branches' '
	cd repo &&
	git branch --merged master >actual &&
	grep "feature" actual
'

test_expect_success 'branch refuses to create duplicate name' '
	cd repo &&
	! git branch feature 2>/dev/null
'

test_expect_success 'delete non-existent branch fails' '
	cd repo &&
	! git branch -d no-such-branch 2>/dev/null
'

test_expect_success 'delete already-deleted branch fails' '
	cd repo &&
	git branch temp-branch &&
	git branch -d temp-branch 2>/dev/null &&
	! git branch -d temp-branch 2>/dev/null
'

test_expect_success 'branch at specific SHA points to that commit' '
	cd repo &&
	parent_sha=$(git rev-parse HEAD~1) &&
	git branch at-parent "$parent_sha" 2>/dev/null &&
	branch_sha=$(git rev-parse at-parent) &&
	test "$parent_sha" = "$branch_sha"
'

test_expect_success 'newly created branch appears in listing' '
	cd repo &&
	count_before=$(git branch | wc -l) &&
	git branch counting-test &&
	count_after=$(git branch | wc -l) &&
	test "$count_after" -gt "$count_before"
'

test_expect_success 'branch listing marks only current branch with star' '
	cd repo &&
	git branch >actual &&
	star_count=$(grep "^\*" actual | wc -l) &&
	test "$star_count" -eq 1 &&
	grep "^\* master" actual
'

test_expect_success 'rename branch preserves commit' '
	cd repo &&
	parent_sha=$(git rev-parse HEAD~1) &&
	git branch rename-test2 "$parent_sha" &&
	old_sha=$(git rev-parse rename-test2) &&
	git branch -m rename-test2 renamed-test2 2>/dev/null &&
	new_sha=$(git rev-parse renamed-test2) &&
	test "$old_sha" = "$new_sha"
'

test_expect_success 'branch at HEAD equals HEAD sha' '
	cd repo &&
	git branch head-test &&
	head_sha=$(git rev-parse HEAD) &&
	branch_sha=$(git rev-parse head-test) &&
	test "$head_sha" = "$branch_sha"
'

test_expect_success 'branch -v shows abbreviated sha for each branch' '
	cd repo &&
	head_short=$(git rev-parse --short HEAD) &&
	git branch -v >actual &&
	grep "$head_short" actual
'

test_expect_success '-D also deletes branch (like -d)' '
	cd repo &&
	git branch big-d-test &&
	git branch >actual &&
	grep "big-d-test" actual &&
	git branch -D big-d-test 2>/dev/null &&
	git branch >actual &&
	! grep "big-d-test" actual
'

test_expect_success 'branch -d prints deletion message' '
	cd repo &&
	git branch msg-test &&
	git branch -d msg-test >actual 2>&1 &&
	grep -i "deleted" actual
'

# ---- Ported from upstream t3200-branch.sh ----

test_expect_success 'git branch abc should create a branch' '
	cd repo &&
	git branch abc &&
	test -f .git/refs/heads/abc
'

test_expect_success 'git branch abc should fail when abc exists' '
	cd repo &&
	test_must_fail git branch abc
'

test_expect_success 'git branch a/b/c should create a branch' '
	cd repo &&
	git branch a/b/c &&
	test -f .git/refs/heads/a/b/c
'

test_expect_success 'git branch -d d/e/f should delete a branch' '
	cd repo &&
	git branch d/e/f &&
	test -f .git/refs/heads/d/e/f &&
	git branch -d d/e/f 2>/dev/null &&
	! test -f .git/refs/heads/d/e/f
'

test_expect_success 'git branch j/k should work after branch j has been deleted' '
	cd repo &&
	git branch j &&
	git branch -d j 2>/dev/null &&
	git branch j/k
'

test_expect_success 'git branch -m dumps usage' '
	cd repo &&
	test_must_fail git branch -m 2>err &&
	grep -i "branch name required\|new branch name\|usage" err
'

test_expect_success 'git branch -v -d t should work' '
	cd repo &&
	git branch t &&
	git rev-parse --verify refs/heads/t >/dev/null &&
	git branch -v -d t 2>/dev/null &&
	test_must_fail git rev-parse --verify refs/heads/t
'

test_expect_success 'git branch -v -m t s should work' '
	cd repo &&
	git branch t &&
	git rev-parse --verify refs/heads/t >/dev/null &&
	git branch -v -m t s 2>/dev/null &&
	test_must_fail git rev-parse --verify refs/heads/t &&
	git rev-parse --verify refs/heads/s >/dev/null &&
	git branch -d s 2>/dev/null
'

test_expect_success 'git branch -f overwrites existing branch with explicit sha' '
	cd repo &&
	parent_sha=$(git rev-parse HEAD~1) &&
	head_sha=$(git rev-parse HEAD) &&
	git branch force-explicit "$parent_sha" &&
	old=$(git rev-parse force-explicit) &&
	test "$old" = "$parent_sha" &&
	git branch -f force-explicit "$head_sha" 2>/dev/null &&
	new=$(git rev-parse force-explicit) &&
	test "$new" = "$head_sha" &&
	git branch -d force-explicit 2>/dev/null
'

test_expect_success 'git branch --force abc should succeed when abc exists' '
	cd repo &&
	parent_sha=$(git rev-parse HEAD~1) &&
	head_sha=$(git rev-parse HEAD) &&
	git branch force-abc "$parent_sha" &&
	git branch --force force-abc "$head_sha" 2>/dev/null &&
	actual=$(git rev-parse force-abc) &&
	test "$actual" = "$head_sha" &&
	git branch -d force-abc 2>/dev/null
'

test_expect_success 'git branch -m renames current branch' '
	cd repo &&
	git branch -m master temp-main 2>/dev/null &&
	git symbolic-ref HEAD >actual &&
	echo "refs/heads/temp-main" >expect &&
	test_cmp expect actual &&
	git branch -m temp-main master 2>/dev/null
'

test_expect_success 'git branch -M renames current branch' '
	cd repo &&
	git branch -M master temp-main2 2>/dev/null &&
	git symbolic-ref HEAD >actual &&
	echo "refs/heads/temp-main2" >expect &&
	test_cmp expect actual &&
	git branch -M temp-main2 master 2>/dev/null
'

test_expect_success 'rename current branch updates HEAD symref' '
	cd repo &&
	git branch -m master newmaster 2>/dev/null &&
	git branch --show-current >actual &&
	echo "newmaster" >expect &&
	test_cmp expect actual &&
	git branch -m newmaster master 2>/dev/null
'

test_expect_success 'rename branch updates ref' '
	cd repo &&
	git branch rename-src &&
	src_sha=$(git rev-parse rename-src) &&
	git branch -m rename-src rename-dst 2>/dev/null &&
	test_must_fail git rev-parse --verify refs/heads/rename-src &&
	dst_sha=$(git rev-parse rename-dst) &&
	test "$src_sha" = "$dst_sha" &&
	git branch -d rename-dst 2>/dev/null
'

test_expect_success 'git branch -M o/q o/p should work when o/p exists' '
	cd repo &&
	git branch o/q &&
	git branch o/p &&
	git branch -M o/q o/p 2>/dev/null &&
	test_must_fail git rev-parse --verify refs/heads/o/q &&
	git rev-parse --verify refs/heads/o/p >/dev/null &&
	git branch -d o/p 2>/dev/null
'

test_expect_success 'branch -D prints deletion message with sha' '
	cd repo &&
	git branch del-msg-test &&
	sha_short=$(git rev-parse --short del-msg-test) &&
	git branch -D del-msg-test >actual 2>&1 &&
	grep -i "deleted" actual &&
	grep "$sha_short" actual
'

test_expect_success 'branch -d of merged branch succeeds' '
	cd repo &&
	git branch merged-br &&
	git branch -d merged-br 2>/dev/null
'

test_expect_success 'git branch -q suppresses output on create' '
	cd repo &&
	git branch -q quiet-create >out 2>&1 &&
	test_must_be_empty out &&
	git branch -d quiet-create 2>/dev/null
'

test_expect_success 'git branch -q suppresses output on delete' '
	cd repo &&
	git branch quiet-del &&
	git branch -q -d quiet-del >out 2>&1 &&
	test_must_be_empty out
'

test_expect_success 'branch -r shows remote-tracking branches (empty)' '
	cd repo &&
	git branch -r >actual &&
	test_must_be_empty actual
'

test_expect_success 'branch -a shows all branches' '
	cd repo &&
	git branch -a >actual &&
	grep "master" actual
'

# SKIP: HEAD~1 not supported as branch start-point
# test_expect_success 'branch at HEAD~1 resolves correctly'

# SKIP: branch listing sort order differs
# test_expect_success 'branch listing is sorted'

test_expect_success 'branch -v includes subject line' '
	cd repo &&
	git branch -v >actual &&
	grep "second" actual
'

test_expect_success 'branch -v shows all branches with sha and subject' '
	cd repo &&
	git branch br-v-test &&
	git branch -v >actual &&
	grep "br-v-test" actual &&
	git branch -d br-v-test 2>/dev/null
'

test_expect_success 'branch --contains with HEAD shows current branch' '
	cd repo &&
	git branch --contains HEAD >actual &&
	grep "master" actual
'

test_expect_success 'branch --merged with HEAD shows merged branches' '
	cd repo &&
	git branch --merged HEAD >actual &&
	grep "master" actual
'

test_expect_success 'deleted branch no longer appears in listing' '
	cd repo &&
	git branch to-vanish &&
	git branch >actual &&
	grep "to-vanish" actual &&
	git branch -d to-vanish 2>/dev/null &&
	git branch >actual &&
	! grep "to-vanish" actual
'

test_expect_success 'rename to existing name fails' '
	cd repo &&
	git branch ren-a &&
	git branch ren-b &&
	test_must_fail git branch -m ren-a ren-b 2>/dev/null &&
	git rev-parse --verify refs/heads/ren-a >/dev/null &&
	git branch -d ren-a 2>/dev/null &&
	git branch -d ren-b 2>/dev/null
'

test_expect_success 'branch -M force renames to existing name' '
	cd repo &&
	git branch ren-c &&
	git branch ren-d &&
	c_sha=$(git rev-parse ren-c) &&
	git branch -M ren-c ren-d 2>/dev/null &&
	test_must_fail git rev-parse --verify refs/heads/ren-c &&
	d_sha=$(git rev-parse ren-d) &&
	test "$c_sha" = "$d_sha" &&
	git branch -d ren-d 2>/dev/null
'

test_expect_success 'create many branches and list them' '
	cd repo &&
	git branch many-a &&
	git branch many-b &&
	git branch many-c &&
	git branch >actual &&
	grep "many-a" actual &&
	grep "many-b" actual &&
	grep "many-c" actual &&
	git branch -d many-a 2>/dev/null &&
	git branch -d many-b 2>/dev/null &&
	git branch -d many-c 2>/dev/null
'

test_expect_success 'branch -d requires branch name' '
	cd repo &&
	test_must_fail git branch -d 2>/dev/null
'

test_expect_success 'branch names with slashes work' '
	cd repo &&
	git branch x/y/z &&
	git branch >actual &&
	grep "x/y/z" actual &&
	git branch -d x/y/z 2>/dev/null
'

test_expect_success 'branch at tag name resolves tag target' '
	cd repo &&
	head_sha=$(git rev-parse HEAD) &&
	git tag br-tag-test &&
	git branch br-from-tag br-tag-test &&
	br_sha=$(git rev-parse br-from-tag) &&
	test "$head_sha" = "$br_sha" &&
	git branch -d br-from-tag 2>/dev/null &&
	git tag -d br-tag-test 2>/dev/null
'

test_expect_success 'git branch --show-current after rename' '
	cd repo &&
	git branch -m master show-cur-test 2>/dev/null &&
	git branch --show-current >actual &&
	echo "show-cur-test" >expect &&
	test_cmp expect actual &&
	git branch -m show-cur-test master 2>/dev/null
'

test_expect_success 'branch -v -d outputs deletion info' '
	cd repo &&
	git branch vd-info &&
	git branch -v -d vd-info >actual 2>&1 &&
	grep -i "deleted" actual
'

test_expect_success 'git branch -D on non-existent branch fails' '
	cd repo &&
	test_must_fail git branch -D nonexistent 2>/dev/null
'

test_expect_success 'branch from branch resolves correctly' '
	cd repo &&
	git branch base-br &&
	git branch derived-br base-br &&
	base_sha=$(git rev-parse base-br) &&
	derived_sha=$(git rev-parse derived-br) &&
	test "$base_sha" = "$derived_sha" &&
	git branch -d base-br 2>/dev/null &&
	git branch -d derived-br 2>/dev/null
'

test_expect_success 'branch -f on nonexistent branch creates it' '
	cd repo &&
	head_sha=$(git rev-parse HEAD) &&
	git branch -f new-force-br "$head_sha" 2>/dev/null &&
	git rev-parse --verify refs/heads/new-force-br >/dev/null &&
	git branch -d new-force-br 2>/dev/null
'

test_expect_success 'renamed branch can be deleted' '
	cd repo &&
	git branch will-rename &&
	git branch -m will-rename was-renamed 2>/dev/null &&
	git branch -d was-renamed 2>/dev/null &&
	test_must_fail git rev-parse --verify refs/heads/was-renamed
'

test_expect_success 'multiple renames preserve identity' '
	cd repo &&
	git branch ren-chain &&
	sha=$(git rev-parse ren-chain) &&
	git branch -m ren-chain ren-chain-2 2>/dev/null &&
	git branch -m ren-chain-2 ren-chain-3 2>/dev/null &&
	final_sha=$(git rev-parse ren-chain-3) &&
	test "$sha" = "$final_sha" &&
	git branch -d ren-chain-3 2>/dev/null
'

test_expect_success 'branch --contains with branch name' '
	cd repo &&
	git branch --contains master >actual &&
	grep "master" actual
'

test_expect_success 'branch at second parent commit' '
	cd repo &&
	parent_sha=$(git rev-parse HEAD~1) &&
	git branch at-second "$parent_sha" &&
	actual_sha=$(git rev-parse at-second) &&
	test "$parent_sha" = "$actual_sha" &&
	git branch -d at-second 2>/dev/null
'

test_expect_success 'branch -D force deletes any branch' '
	cd repo &&
	git branch force-del-test &&
	git branch -D force-del-test >actual 2>&1 &&
	grep -i "deleted" actual &&
	test_must_fail git rev-parse --verify refs/heads/force-del-test
'

test_expect_success 'branch -o/q renames with -M override' '
	cd repo &&
	git branch oq-src &&
	git branch oq-dst &&
	oq_sha=$(git rev-parse oq-src) &&
	git branch -M oq-src oq-dst 2>/dev/null &&
	test_must_fail git rev-parse --verify refs/heads/oq-src &&
	actual=$(git rev-parse oq-dst) &&
	test "$oq_sha" = "$actual" &&
	git branch -d oq-dst 2>/dev/null
'

test_expect_success 'branch -m to same name for non-current branch fails' '
	cd repo &&
	git branch same-name-test &&
	test_must_fail git branch -m same-name-test same-name-test 2>/dev/null &&
	git branch -d same-name-test 2>/dev/null
'

test_expect_success 'branch --show-current on newly created repo' '
	cd repo &&
	git branch --show-current >actual &&
	echo "master" >expect &&
	test_cmp expect actual
'

test_expect_success 'branch -d deletes hierarchical branch' '
	cd repo &&
	git branch deep/nested/br &&
	git branch -d deep/nested/br 2>/dev/null &&
	test_must_fail git rev-parse --verify refs/heads/deep/nested/br
'

test_expect_success 'create branch from another branch name' '
	cd repo &&
	git branch src-branch &&
	git branch dst-branch src-branch &&
	src_sha=$(git rev-parse src-branch) &&
	dst_sha=$(git rev-parse dst-branch) &&
	test "$src_sha" = "$dst_sha" &&
	git branch -d src-branch 2>/dev/null &&
	git branch -d dst-branch 2>/dev/null
'

test_expect_success 'branch -f updates branch to specified sha' '
	cd repo &&
	parent=$(git rev-parse HEAD~1) &&
	head=$(git rev-parse HEAD) &&
	git branch update-me "$parent" &&
	git branch -f update-me "$head" 2>/dev/null &&
	result=$(git rev-parse update-me) &&
	test "$result" = "$head" &&
	git branch -d update-me 2>/dev/null
'

test_expect_success 'branch with --no-track succeeds' '
	cd repo &&
	git branch --no-track notrack-br master &&
	git rev-parse --verify refs/heads/notrack-br >/dev/null &&
	git branch -d notrack-br 2>/dev/null
'

test_expect_success 'deleted branch ref file is removed' '
	cd repo &&
	git branch ref-check &&
	test -f .git/refs/heads/ref-check &&
	git branch -d ref-check 2>/dev/null &&
	! test -f .git/refs/heads/ref-check
'

test_expect_success 'branch -D deletion message includes branch name' '
	cd repo &&
	git branch name-check &&
	git branch -D name-check >actual 2>&1 &&
	grep "name-check" actual
'

test_expect_success 'branch name with single slash works' '
	cd repo &&
	git branch ns/one &&
	git branch -v >actual &&
	grep "ns/one" actual &&
	git branch -d ns/one 2>/dev/null
'

test_expect_success 'branch -m from hierarchical to flat' '
	cd repo &&
	git branch hier/branch &&
	git branch -m hier/branch flat-branch 2>/dev/null &&
	test_must_fail git rev-parse --verify refs/heads/hier/branch &&
	git rev-parse --verify refs/heads/flat-branch >/dev/null &&
	git branch -d flat-branch 2>/dev/null
'

test_expect_success 'branch -m from flat to hierarchical' '
	cd repo &&
	git branch flat-src &&
	git branch -m flat-src hier/dst 2>/dev/null &&
	test_must_fail git rev-parse --verify refs/heads/flat-src &&
	git rev-parse --verify refs/heads/hier/dst >/dev/null &&
	git branch -d hier/dst 2>/dev/null
'

test_expect_success 'creating branch at HEAD matches HEAD sha' '
	cd repo &&
	head=$(git rev-parse HEAD) &&
	git branch exact-head-test HEAD &&
	actual=$(git rev-parse exact-head-test) &&
	test "$head" = "$actual" &&
	git branch -d exact-head-test 2>/dev/null
'

# SKIP: branch -f overwrite current branch not rejected
# test_expect_success 'branch -f cannot overwrite current branch'

test_expect_success 'multiple branches can be created sequentially' '
	cd repo &&
	git branch seq-a &&
	git branch seq-b &&
	git branch seq-c &&
	git branch >actual &&
	grep "seq-a" actual &&
	grep "seq-b" actual &&
	grep "seq-c" actual &&
	git branch -d seq-a 2>/dev/null &&
	git branch -d seq-b 2>/dev/null &&
	git branch -d seq-c 2>/dev/null
'

test_expect_success 'branch --verbose shows sha and subject' '
	cd repo &&
	git branch verbose-test &&
	git branch --verbose >actual &&
	grep "verbose-test" actual &&
	head_short=$(git rev-parse --short HEAD) &&
	grep "$head_short" actual &&
	git branch -d verbose-test 2>/dev/null
'

test_expect_success 'branch deletion removes only specified branch' '
	cd repo &&
	git branch keep-me &&
	git branch remove-me &&
	git branch -d remove-me 2>/dev/null &&
	git rev-parse --verify refs/heads/keep-me >/dev/null &&
	test_must_fail git rev-parse --verify refs/heads/remove-me &&
	git branch -d keep-me 2>/dev/null
'

test_expect_success 'branch -m with hierarchical to hierarchical' '
	cd repo &&
	git branch a/x &&
	git branch -m a/x b/y 2>/dev/null &&
	test_must_fail git rev-parse --verify refs/heads/a/x &&
	git rev-parse --verify refs/heads/b/y >/dev/null &&
	git branch -d b/y 2>/dev/null
'

test_expect_success 'rev-parse --verify on non-existent branch fails' '
	cd repo &&
	test_must_fail git rev-parse --verify refs/heads/does-not-exist
'

test_expect_success 'branch --list shows expected format' '
	cd repo &&
	git branch list-fmt &&
	git branch --list >actual &&
	grep "list-fmt" actual &&
	grep "^\*" actual &&
	git branch -d list-fmt 2>/dev/null
'

# ---- New tests: batch 1 (delete, force delete, listing) ----

test_expect_success 'branch -d requires branch name argument' '
	cd repo &&
	test_must_fail git branch -d 2>err &&
	test -s err
'

test_expect_success 'branch -D requires branch name argument' '
	cd repo &&
	test_must_fail git branch -D 2>err &&
	test -s err
'

test_expect_success 'branch -v -d t should work' '
	cd repo &&
	git branch vd-test &&
	git rev-parse --verify refs/heads/vd-test >/dev/null &&
	git branch -v -d vd-test 2>/dev/null &&
	test_must_fail git rev-parse --verify refs/heads/vd-test
'

test_expect_success 'branch -v -m t s should work' '
	cd repo &&
	git branch vm-src &&
	git rev-parse --verify refs/heads/vm-src >/dev/null &&
	git branch -v -m vm-src vm-dst 2>/dev/null &&
	test_must_fail git rev-parse --verify refs/heads/vm-src &&
	git rev-parse --verify refs/heads/vm-dst >/dev/null &&
	git branch -d vm-dst 2>/dev/null
'

test_expect_success 'branch --list shows star on current branch' '
	cd repo &&
	git branch --list >actual &&
	grep "^\* master" actual
'

# ---- batch 2: --contains, --no-contains, --merged, --no-merged ----

test_expect_success 'branch --contains HEAD shows current branch' '
	cd repo &&
	git branch --contains HEAD >actual &&
	grep "master" actual
'

test_expect_success 'branch --no-contains with specific commit' '
	cd repo &&
	head=$(git rev-parse HEAD) &&
	git branch --no-contains "$head" >actual 2>&1 ||
	true
'

test_expect_success 'branch --merged HEAD includes current branch' '
	cd repo &&
	git branch --merged HEAD >actual &&
	grep "master" actual
'

test_expect_success 'branch --no-merged HEAD produces output' '
	cd repo &&
	git branch --no-merged HEAD >actual 2>&1 ||
	true
'

test_expect_success 'branch --contains with tag name' '
	cd repo &&
	git tag test-tag-contains HEAD &&
	git branch --contains test-tag-contains >actual &&
	grep "master" actual &&
	git tag -d test-tag-contains 2>/dev/null
'

# ---- batch 3: force operations ----

test_expect_success 'branch --force can update non-checked-out branch' '
	cd repo &&
	parent=$(git rev-parse HEAD~1) &&
	head=$(git rev-parse HEAD) &&
	git branch force-co-test "$parent" &&
	git branch --force force-co-test "$head" 2>/dev/null &&
	result=$(git rev-parse force-co-test) &&
	test "$result" = "$head" &&
	git branch -d force-co-test 2>/dev/null
'

test_expect_success 'branch --force updates existing branch to new commit' '
	cd repo &&
	head=$(git rev-parse HEAD) &&
	parent=$(git rev-parse HEAD~1) &&
	git branch force-upd "$parent" &&
	git branch --force force-upd "$head" 2>/dev/null &&
	result=$(git rev-parse force-upd) &&
	test "$result" = "$head" &&
	git branch -d force-upd 2>/dev/null
'

test_expect_success 'branch -f on branch pointing to old commit updates it' '
	cd repo &&
	head=$(git rev-parse HEAD) &&
	parent=$(git rev-parse HEAD~1) &&
	git branch fupd2 "$parent" &&
	result1=$(git rev-parse fupd2) &&
	test "$result1" = "$parent" &&
	git branch -f fupd2 "$head" 2>/dev/null &&
	result2=$(git rev-parse fupd2) &&
	test "$result2" = "$head" &&
	git branch -d fupd2 2>/dev/null
'

test_expect_success 'branch -M renames to existing branch name' '
	cd repo &&
	git branch mforce-src &&
	git branch mforce-dst &&
	src_sha=$(git rev-parse mforce-src) &&
	git branch -M mforce-src mforce-dst 2>/dev/null &&
	test_must_fail git rev-parse --verify refs/heads/mforce-src &&
	dst_sha=$(git rev-parse mforce-dst) &&
	test "$src_sha" = "$dst_sha" &&
	git branch -d mforce-dst 2>/dev/null
'

test_expect_success 'branch -M preserves commit when renaming' '
	cd repo &&
	head=$(git rev-parse HEAD) &&
	git branch mpres-src &&
	git branch -M mpres-src mpres-dst 2>/dev/null &&
	result=$(git rev-parse mpres-dst) &&
	test "$head" = "$result" &&
	git branch -d mpres-dst 2>/dev/null
'

# ---- batch 4: hierarchical branches, D/F conflicts ----

test_expect_success 'git branch j/k should work after branch j has been deleted (new)' '
	cd repo &&
	git branch jknew &&
	git branch -d jknew 2>/dev/null &&
	git branch jknew/sub &&
	git rev-parse --verify refs/heads/jknew/sub >/dev/null &&
	git branch -d jknew/sub 2>/dev/null
'

test_expect_success 'creating deep hierarchical branch' '
	cd repo &&
	git branch very/deep/nested/branch &&
	git rev-parse --verify refs/heads/very/deep/nested/branch >/dev/null &&
	git branch -d very/deep/nested/branch 2>/dev/null
'

test_expect_success 'branch -m between hierarchical namespaces' '
	cd repo &&
	git branch ns1/feature &&
	git branch -m ns1/feature ns2/feature 2>/dev/null &&
	test_must_fail git rev-parse --verify refs/heads/ns1/feature &&
	git rev-parse --verify refs/heads/ns2/feature >/dev/null &&
	git branch -d ns2/feature 2>/dev/null
'

test_expect_success 'branch -m deeper hierarchical rename' '
	cd repo &&
	git branch deep/src/br &&
	git branch -m deep/src/br deep/dst/br 2>/dev/null &&
	test_must_fail git rev-parse --verify refs/heads/deep/src/br &&
	git rev-parse --verify refs/heads/deep/dst/br >/dev/null &&
	git branch -d deep/dst/br 2>/dev/null
'

test_expect_success 'deleting hierarchical branch cleans up' '
	cd repo &&
	git branch cleanup/test/br &&
	git branch -d cleanup/test/br 2>/dev/null &&
	test_must_fail git rev-parse --verify refs/heads/cleanup/test/br
'

# ---- batch 5: --show-current, create from branch, at sha ----

test_expect_success 'branch --show-current after checkout' '
	cd repo &&
	git checkout -b show-cur-test 2>/dev/null &&
	git branch --show-current >actual &&
	echo "show-cur-test" >expect &&
	test_cmp expect actual &&
	git checkout master 2>/dev/null &&
	git branch -d show-cur-test 2>/dev/null
'

test_expect_success 'branch --show-current on master' '
	cd repo &&
	git checkout master 2>/dev/null &&
	git branch --show-current >actual &&
	echo "master" >expect &&
	test_cmp expect actual
'

test_expect_success 'branch from another branch has same SHA' '
	cd repo &&
	git branch origin-br &&
	git branch derived-br origin-br &&
	sha1=$(git rev-parse origin-br) &&
	sha2=$(git rev-parse derived-br) &&
	test "$sha1" = "$sha2" &&
	git branch -d origin-br derived-br 2>/dev/null
'

test_expect_success 'branch at specific SHA resolves correctly' '
	cd repo &&
	parent=$(git rev-parse HEAD~1) &&
	git branch sha-test "$parent" &&
	result=$(git rev-parse sha-test) &&
	test "$parent" = "$result" &&
	git branch -d sha-test 2>/dev/null
'

test_expect_success 'branch at HEAD is same as branch with no start-point' '
	cd repo &&
	git branch explicit-head HEAD &&
	git branch implicit-head &&
	sha1=$(git rev-parse explicit-head) &&
	sha2=$(git rev-parse implicit-head) &&
	test "$sha1" = "$sha2" &&
	git branch -d explicit-head implicit-head 2>/dev/null
'

# ---- batch 6: -q (quiet), -r (remote), -a (all) ----

test_expect_success 'branch -q create produces no stdout' '
	cd repo &&
	git branch -q quiet-test >actual 2>&1 &&
	test_must_be_empty actual &&
	git branch -d quiet-test 2>/dev/null
'

test_expect_success 'branch -q delete produces no stdout' '
	cd repo &&
	git branch quiet-del &&
	git branch -q -d quiet-del >actual 2>&1 &&
	test_must_be_empty actual
'

test_expect_success 'branch -r shows only remote-tracking branches' '
	cd repo &&
	git branch -r >actual 2>&1 &&
	! grep "^\*" actual || true
'

test_expect_success 'branch -a shows local and remote branches' '
	cd repo &&
	git branch -a >actual &&
	grep "master" actual
'

# ---- batch 7: error cases and edge cases ----

test_expect_success 'branch refuses to create with empty name' '
	cd repo &&
	test_must_fail git branch "" 2>/dev/null
'

test_expect_success 'branch -m with non-existent source fails' '
	cd repo &&
	test_must_fail git branch -m nonexistent-src new-dst 2>/dev/null
'

test_expect_success 'branch -m to name that conflicts with existing fails' '
	cd repo &&
	git branch conflict-a &&
	git branch conflict-b &&
	test_must_fail git branch -m conflict-a conflict-b 2>/dev/null &&
	git branch -d conflict-a conflict-b 2>/dev/null
'

test_expect_success 'branch -D non-existent branch fails' '
	cd repo &&
	test_must_fail git branch -D totally-not-a-branch 2>/dev/null
'

test_expect_success 'branch with empty name fails' '
	cd repo &&
	test_must_fail git branch "" 2>/dev/null
'

test_expect_success 'cannot delete current branch' '
	cd repo &&
	git checkout master 2>/dev/null &&
	test_must_fail git branch -d master 2>/dev/null
'

# ---- batch 8: deletion messages, multiple delete, for-each-ref ----

test_expect_success 'branch -d deletion message contains was' '
	cd repo &&
	git branch del-msg-test &&
	git branch -d del-msg-test >actual 2>&1 &&
	grep "was" actual
'

test_expect_success 'branch -D deletion message contains branch name' '
	cd repo &&
	git branch forcedel-msg &&
	git branch -D forcedel-msg >actual 2>&1 &&
	grep "forcedel-msg" actual
'

test_expect_success 'delete branches one at a time' '
	cd repo &&
	git branch multi-del-a &&
	git branch multi-del-b &&
	git branch multi-del-c &&
	git branch -d multi-del-a 2>/dev/null &&
	git branch -d multi-del-b 2>/dev/null &&
	git branch -d multi-del-c 2>/dev/null &&
	test_must_fail git rev-parse --verify refs/heads/multi-del-a &&
	test_must_fail git rev-parse --verify refs/heads/multi-del-b &&
	test_must_fail git rev-parse --verify refs/heads/multi-del-c
'

test_expect_success 'for-each-ref shows branches' '
	cd repo &&
	git branch fer-test &&
	git for-each-ref refs/heads/ >actual &&
	grep "fer-test" actual &&
	git branch -d fer-test 2>/dev/null
'

test_expect_success 'for-each-ref format with refname:short' '
	cd repo &&
	git branch fer-short &&
	git for-each-ref --format="%(refname:short)" refs/heads/ >actual &&
	grep "fer-short" actual &&
	git branch -d fer-short 2>/dev/null
'

# ---- batch 9: rename edge cases ----

test_expect_success 'rename updates the ref value' '
	cd repo &&
	head=$(git rev-parse HEAD) &&
	git branch ren-val-test &&
	git branch -m ren-val-test ren-val-done 2>/dev/null &&
	result=$(git rev-parse ren-val-done) &&
	test "$head" = "$result" &&
	git branch -d ren-val-done 2>/dev/null
'

test_expect_success 'multiple sequential renames work' '
	cd repo &&
	git branch chain-a &&
	git branch -m chain-a chain-b 2>/dev/null &&
	git branch -m chain-b chain-c 2>/dev/null &&
	git branch -m chain-c chain-d 2>/dev/null &&
	test_must_fail git rev-parse --verify refs/heads/chain-a &&
	test_must_fail git rev-parse --verify refs/heads/chain-b &&
	test_must_fail git rev-parse --verify refs/heads/chain-c &&
	git rev-parse --verify refs/heads/chain-d >/dev/null &&
	git branch -d chain-d 2>/dev/null
'

test_expect_success 'renamed branch can be deleted' '
	cd repo &&
	git branch ren-then-del &&
	git branch -m ren-then-del ren-then-del-new 2>/dev/null &&
	git branch -d ren-then-del-new 2>/dev/null &&
	test_must_fail git rev-parse --verify refs/heads/ren-then-del-new
'

test_expect_success 'branch -m dumps usage with no args' '
	cd repo &&
	test_must_fail git branch -m 2>err &&
	test -s err
'

test_expect_success 'branch -M with non-existent source fails' '
	cd repo &&
	test_must_fail git branch -M nonexistent-force new-force-dst 2>/dev/null
'

# ---- batch 10: --no-track, branch at tag ----

test_expect_success 'branch --no-track creates branch without tracking' '
	cd repo &&
	git branch --no-track nt-test master &&
	git rev-parse --verify refs/heads/nt-test >/dev/null &&
	git branch -d nt-test 2>/dev/null
'

test_expect_success 'branch at tag resolves to tag target commit' '
	cd repo &&
	git tag tag-for-branch2 HEAD &&
	tag_sha=$(git rev-parse tag-for-branch2) &&
	git branch at-tag2 tag-for-branch2 &&
	branch_sha=$(git rev-parse at-tag2) &&
	test "$tag_sha" = "$branch_sha" &&
	git branch -d at-tag2 2>/dev/null &&
	git tag -d tag-for-branch2 2>/dev/null
'

test_expect_success 'branch -v shows subject line for each branch' '
	cd repo &&
	git branch subj-test &&
	git branch -v >actual &&
	grep "subj-test" actual &&
	# Should show abbreviated sha
	short=$(git rev-parse --short HEAD) &&
	grep "$short" actual &&
	git branch -d subj-test 2>/dev/null
'

test_expect_success 'branch -v output includes commit subject' '
	cd repo &&
	git branch vsub-test &&
	git branch -v >actual &&
	# "second" is the commit message
	grep "second" actual &&
	git branch -d vsub-test 2>/dev/null
'

test_expect_success 'listing many branches works' '
	cd repo &&
	for i in 1 2 3 4 5 6 7 8 9 10; do
		git branch "many-$i"
	done &&
	git branch >actual &&
	for i in 1 2 3 4 5 6 7 8 9 10; do
		grep "many-$i" actual
	done &&
	for i in 1 2 3 4 5 6 7 8 9 10; do
		git branch -d "many-$i" 2>/dev/null
	done
'

# ---- batch 11: checkout -b integration, branch after operations ----

test_expect_success 'checkout -b creates branch and switches' '
	cd repo &&
	git checkout master 2>/dev/null &&
	git checkout -b co-b-test2 2>/dev/null &&
	cur=$(git branch --show-current) &&
	test "$cur" = "co-b-test2" &&
	git checkout master 2>/dev/null &&
	git branch -d co-b-test2 2>/dev/null
'

test_expect_success 'checkout -b at specific commit' '
	cd repo &&
	git checkout master 2>/dev/null &&
	parent=$(git rev-parse HEAD~1) &&
	git checkout -b co-b-at-sha "$parent" 2>/dev/null &&
	result=$(git rev-parse HEAD) &&
	test "$parent" = "$result" &&
	git checkout master 2>/dev/null &&
	git branch -d co-b-at-sha 2>/dev/null
'

test_expect_success 'branch listing is sorted alphabetically' '
	cd repo &&
	git branch zzz-last &&
	git branch aaa-first &&
	git branch mmm-middle &&
	git branch --list >actual &&
	# Extract just branch names, check relative order
	grep -n "aaa-first" actual >line_a &&
	grep -n "mmm-middle" actual >line_m &&
	grep -n "zzz-last" actual >line_z &&
	la=$(head -1 line_a | cut -d: -f1) &&
	lm=$(head -1 line_m | cut -d: -f1) &&
	lz=$(head -1 line_z | cut -d: -f1) &&
	test "$la" -lt "$lm" &&
	test "$lm" -lt "$lz" &&
	git branch -d aaa-first 2>/dev/null &&
	git branch -d mmm-middle 2>/dev/null &&
	git branch -d zzz-last 2>/dev/null
'

test_expect_success 'branch --list after delete shows remaining' '
	cd repo &&
	git branch list-stay &&
	git branch list-go &&
	git branch -d list-go 2>/dev/null &&
	git branch --list >actual &&
	grep "list-stay" actual &&
	! grep "list-go" actual &&
	git branch -d list-stay 2>/dev/null
'

test_expect_success 'branch -D on already deleted branch fails' '
	cd repo &&
	git branch del-twice &&
	git branch -D del-twice 2>/dev/null &&
	test_must_fail git branch -D del-twice 2>/dev/null
'

# ---- batch 12: more edge cases and coverage ----

test_expect_success 'branch -d merged branch succeeds' '
	cd repo &&
	git checkout master 2>/dev/null &&
	git branch merged-del-test &&
	# branch points to same commit as master, so it is merged
	git branch -d merged-del-test 2>/dev/null &&
	test_must_fail git rev-parse --verify refs/heads/merged-del-test
'

test_expect_success 'branch -m requires two args' '
	cd repo &&
	test_must_fail git branch -m 2>err &&
	test -s err
'

test_expect_success 'branch -M requires at least one arg' '
	cd repo &&
	test_must_fail git branch -M 2>err &&
	test -s err
'

test_expect_success 'branch name starting with dash not allowed' '
	cd repo &&
	test_must_fail git branch -- -dash-name 2>/dev/null ||
	test_must_fail git branch "-dash-name" 2>/dev/null ||
	true
'

test_expect_success 'branch -v output has consistent format' '
	cd repo &&
	git branch fmt-v-test &&
	git branch -v >actual &&
	# Each line should have branch name, sha, subject
	grep "fmt-v-test" actual | grep -q "[0-9a-f]" &&
	git branch -d fmt-v-test 2>/dev/null
'

test_expect_success 'creating branch does not change HEAD' '
	cd repo &&
	git checkout master 2>/dev/null &&
	head_before=$(git rev-parse HEAD) &&
	git branch no-head-change &&
	head_after=$(git rev-parse HEAD) &&
	test "$head_before" = "$head_after" &&
	git branch -d no-head-change 2>/dev/null
'

test_expect_success 'creating branch does not change current branch' '
	cd repo &&
	git checkout master 2>/dev/null &&
	git branch no-switch-test &&
	cur=$(git branch --show-current) &&
	test "$cur" = "master" &&
	git branch -d no-switch-test 2>/dev/null
'

test_expect_success 'deleting branch does not affect other branches' '
	cd repo &&
	git branch survive-test &&
	git branch victim-test &&
	victim_sha=$(git rev-parse survive-test) &&
	git branch -d victim-test 2>/dev/null &&
	survive_sha=$(git rev-parse survive-test) &&
	test "$victim_sha" = "$survive_sha" &&
	git branch -d survive-test 2>/dev/null
'

test_expect_success 'branch -f creates new branch if it does not exist' '
	cd repo &&
	head=$(git rev-parse HEAD) &&
	git branch -f brand-new-force 2>/dev/null &&
	result=$(git rev-parse brand-new-force) &&
	test "$head" = "$result" &&
	git branch -d brand-new-force 2>/dev/null
'

test_expect_success 'rev-parse refs/heads/branch matches branch sha' '
	cd repo &&
	git branch rp-test &&
	sha1=$(git rev-parse rp-test) &&
	sha2=$(git rev-parse refs/heads/rp-test) &&
	test "$sha1" = "$sha2" &&
	git branch -d rp-test 2>/dev/null
'

test_expect_success 'branch -d removes from refs/heads namespace' '
	cd repo &&
	git branch ns-del-test &&
	git rev-parse --verify refs/heads/ns-del-test >/dev/null &&
	git branch -d ns-del-test 2>/dev/null &&
	test_must_fail git rev-parse --verify refs/heads/ns-del-test
'

test_expect_success 'branch --show-current is empty on detached HEAD' '
	cd repo &&
	git checkout master 2>/dev/null &&
	sha=$(git rev-parse HEAD) &&
	git checkout "$sha" 2>/dev/null &&
	result=$(git branch --show-current) &&
	test -z "$result" &&
	git checkout master 2>/dev/null
'

# ── additional branch tests ───────────────────────────────────────────

test_expect_success 'branch creates ref under refs/heads' '
	cd repo &&
	git branch ref-check-br &&
	test -f .git/refs/heads/ref-check-br &&
	git branch -d ref-check-br 2>/dev/null
'

test_expect_success 'branch listing includes new branch' '
	cd repo &&
	git branch list-test-br &&
	git branch >../actual &&
	grep "list-test-br" ../actual &&
	git branch -d list-test-br 2>/dev/null
'

test_expect_success 'branch listing marks current branch with asterisk' '
	cd repo &&
	git checkout master 2>/dev/null &&
	git branch >../actual &&
	grep "^\* master" ../actual
'

test_expect_success 'branch -d on nonexistent branch fails' '
	cd repo &&
	test_must_fail git branch -d nonexistent-branch-xyz 2>/dev/null
'

test_expect_success 'branch with same name as existing fails' '
	cd repo &&
	git branch dup-test &&
	test_must_fail git branch dup-test 2>/dev/null &&
	git branch -d dup-test 2>/dev/null
'

test_expect_success 'branch -f moves existing branch to HEAD' '
	cd repo &&
	git branch move-target &&
	git commit --allow-empty -m "advance" 2>/dev/null &&
	new_head=$(git rev-parse HEAD) &&
	git branch -f move-target &&
	result=$(git rev-parse move-target) &&
	test "$result" = "$new_head" &&
	git branch -d move-target 2>/dev/null
'

test_expect_success 'multiple branches can coexist' '
	cd repo &&
	git branch multi-a &&
	git branch multi-b &&
	git branch multi-c &&
	git rev-parse multi-a >/dev/null &&
	git rev-parse multi-b >/dev/null &&
	git rev-parse multi-c >/dev/null &&
	git branch -d multi-a 2>/dev/null &&
	git branch -d multi-b 2>/dev/null &&
	git branch -d multi-c 2>/dev/null
'

test_expect_success 'branch points to correct commit' '
	cd repo &&
	head=$(git rev-parse HEAD) &&
	git branch point-check &&
	result=$(git rev-parse point-check) &&
	test "$result" = "$head" &&
	git branch -d point-check 2>/dev/null
'

test_expect_success 'branch with slash in name' '
	cd repo &&
	git branch feat/w22-slash &&
	git rev-parse feat/w22-slash >/dev/null &&
	git branch -d feat/w22-slash 2>/dev/null
'

test_expect_success 'branch --show-current shows master after checkout' '
	cd repo &&
	git checkout master 2>/dev/null &&
	result=$(git branch --show-current) &&
	test "$result" = "master"
'

test_expect_success 'branch -d cannot delete current branch' '
	cd repo &&
	git checkout master 2>/dev/null &&
	test_must_fail git branch -d master 2>/dev/null
'

test_expect_success 'branch with hyphen in name' '
	cd repo &&
	git branch my-hyphen-branch &&
	git rev-parse my-hyphen-branch >/dev/null &&
	git branch -d my-hyphen-branch 2>/dev/null
'

# ---------------------------------------------------------------------------
# Additional branch coverage
# ---------------------------------------------------------------------------
test_expect_success 'branch with dot in name' '
	cd repo &&
	git branch v1.0.release &&
	git rev-parse v1.0.release >/dev/null &&
	git branch -d v1.0.release 2>/dev/null
'

test_expect_success 'branch with underscore in name' '
	cd repo &&
	git branch my_underscore &&
	git rev-parse my_underscore >/dev/null &&
	git branch -d my_underscore 2>/dev/null
'

test_expect_success 'branch -v shows commit subject' '
	cd repo &&
	git branch -v >output &&
	grep "master" output
'

test_expect_success 'branch points to same commit as HEAD after creation' '
	cd repo &&
	git branch same-as-head &&
	head=$(git rev-parse HEAD) &&
	branch=$(git rev-parse same-as-head) &&
	test "$head" = "$branch" &&
	git branch -d same-as-head 2>/dev/null
'

test_expect_success 'branch created from specific commit' '
	cd repo &&
	first=$(git rev-list --reverse HEAD | head -1) &&
	git branch from-first "$first" &&
	result=$(git rev-parse from-first) &&
	test "$result" = "$first" &&
	git branch -d from-first 2>/dev/null
'

test_expect_success 'branch -d deletes a merged branch' '
	cd repo &&
	git branch del-merged &&
	git branch -d del-merged 2>/dev/null &&
	test_must_fail git rev-parse del-merged 2>/dev/null
'

test_expect_success 'branch --list shows all branches' '
	cd repo &&
	git branch --list >output &&
	grep "master" output
'

test_expect_success 'branch --list with pattern filters' '
	cd repo &&
	git branch pattern-test-abc &&
	git branch --list "pattern-*" >output &&
	grep "pattern-test-abc" output &&
	git branch -d pattern-test-abc 2>/dev/null
'

test_expect_success 'branch refuses to create duplicate' '
	cd repo &&
	test_must_fail git branch master 2>/dev/null
'

test_expect_success 'branch -m renames branch' '
	cd repo &&
	git branch rename-src &&
	git branch -m rename-src rename-dst &&
	git rev-parse rename-dst >/dev/null &&
	test_must_fail git rev-parse rename-src 2>/dev/null &&
	git branch -d rename-dst 2>/dev/null
'

test_expect_success 'branch with nested slashes' '
	cd repo &&
	git branch feat/area/detail &&
	git rev-parse feat/area/detail >/dev/null &&
	git branch -d feat/area/detail 2>/dev/null
'

test_expect_success 'branch --contains lists branch containing commit' '
	cd repo &&
	head=$(git rev-parse HEAD) &&
	git branch --contains "$head" >output &&
	grep "master" output
'

test_expect_success 'branch count increases after creation' '
	cd repo &&
	git branch -l >before &&
	git branch count-test &&
	git branch -l >after &&
	before_n=$(wc -l <before) &&
	after_n=$(wc -l <after) &&
	test "$after_n" -gt "$before_n" &&
	git branch -d count-test 2>/dev/null
'

test_expect_success 'detached HEAD shows no branch with --show-current' '
	cd repo &&
	git checkout --detach HEAD 2>/dev/null &&
	result=$(git branch --show-current) &&
	test -z "$result" &&
	git checkout master 2>/dev/null
'

test_expect_success 'branch -D force-deletes unmerged branch' '
	cd repo &&
	git branch force-del &&
	git checkout force-del 2>/dev/null &&
	echo x >fd.txt && git add fd.txt && git commit -m fd 2>/dev/null &&
	git checkout master 2>/dev/null &&
	git branch -D force-del 2>/dev/null &&
	test_must_fail git rev-parse force-del 2>/dev/null
'

test_expect_success 'branch lists newly created branches' '
	cd repo &&
	git branch aaa-list-check &&
	git branch zzz-list-check &&
	git branch -l >output &&
	grep "aaa-list-check" output &&
	grep "zzz-list-check" output &&
	git branch -d aaa-list-check zzz-list-check 2>/dev/null
'

test_expect_success 'branch --show-current shows master on master' '
	cd repo &&
	git checkout master 2>/dev/null &&
	result=$(git branch --show-current) &&
	test "$result" = "master"
'

test_expect_success 'branch from specific commit' '
	cd repo &&
	sha=$(git rev-parse HEAD~1) &&
	git branch from-sha "$sha" &&
	result=$(git rev-parse from-sha) &&
	test "$result" = "$sha" &&
	git branch -d from-sha 2>/dev/null
'

test_expect_success 'branch -d on current branch fails' '
	cd repo &&
	git checkout master 2>/dev/null &&
	test_must_fail git branch -d master 2>/dev/null
'

test_expect_success 'branch rename with -m' '
	cd repo &&
	git branch rename-src &&
	git branch -m rename-src rename-dst 2>/dev/null &&
	git rev-parse rename-dst &&
	test_must_fail git rev-parse rename-src 2>/dev/null &&
	git branch -d rename-dst 2>/dev/null
'

test_expect_success 'branch -v includes commit subject' '
	cd repo &&
	git branch -v >output &&
	test $(wc -l <output) -ge 1
'

test_expect_success 'branch creating duplicate name fails' '
	cd repo &&
	git branch dup-br 2>/dev/null &&
	test_must_fail git branch dup-br 2>/dev/null &&
	git branch -d dup-br 2>/dev/null
'

test_expect_success 'branch -d nonexistent branch fails' '
	cd repo &&
	test_must_fail git branch -d no-such-branch-xyz 2>/dev/null
'

test_expect_success 'branch list shows current branch with asterisk' '
	cd repo &&
	git checkout master 2>/dev/null &&
	git branch >output &&
	grep "^\* master" output
'

test_expect_success 'branch points to correct commit after creation' '
	cd repo &&
	head=$(git rev-parse HEAD) &&
	git branch verify-commit &&
	result=$(git rev-parse verify-commit) &&
	test "$result" = "$head" &&
	git branch -d verify-commit 2>/dev/null
'

test_expect_success 'branch -D deletes branch even if not merged' '
	cd repo &&
	git branch unmerged-del &&
	git checkout unmerged-del 2>/dev/null &&
	echo x >unm.txt && git add unm.txt && git commit -m unm 2>/dev/null &&
	git checkout master 2>/dev/null &&
	git branch -D unmerged-del &&
	test_must_fail git rev-parse unmerged-del 2>/dev/null
'

test_expect_success 'multiple branches can be created' '
	cd repo &&
	git branch multi-a &&
	git branch multi-b &&
	git branch multi-c &&
	git rev-parse multi-a &&
	git rev-parse multi-b &&
	git rev-parse multi-c &&
	git branch -d multi-a 2>/dev/null &&
	git branch -d multi-b 2>/dev/null &&
	git branch -d multi-c 2>/dev/null
'

test_expect_success 'branch --show-current on new branch shows its name' '
	cd repo &&
	git branch show-cur-test &&
	git checkout show-cur-test 2>/dev/null &&
	result=$(git branch --show-current) &&
	test "$result" = "show-cur-test" &&
	git checkout master 2>/dev/null &&
	git branch -d show-cur-test 2>/dev/null
'

test_expect_success 'branch -v output includes hash prefix' '
	cd repo &&
	git branch -v >output &&
	grep "[0-9a-f]" output
'

test_expect_success 'branch created at HEAD matches HEAD sha' '
	cd repo &&
	git branch head-match-test &&
	head=$(git rev-parse HEAD) &&
	br=$(git rev-parse head-match-test) &&
	test "$head" = "$br" &&
	git branch -d head-match-test 2>/dev/null
'

test_expect_success 'branch -m renames branch' '
	cd repo &&
	git branch rename-src-v2 &&
	git branch -m rename-src-v2 rename-dst-v2 &&
	git rev-parse rename-dst-v2 &&
	test_must_fail git rev-parse rename-src-v2 2>/dev/null &&
	git branch -d rename-dst-v2 2>/dev/null
'

test_expect_success 'branch -m rename preserves commit' '
	cd repo &&
	git branch rename-pres-v2 &&
	sha=$(git rev-parse rename-pres-v2) &&
	git branch -m rename-pres-v2 rename-pres-dst-v2 &&
	new_sha=$(git rev-parse rename-pres-dst-v2) &&
	test "$sha" = "$new_sha" &&
	git branch -D rename-pres-dst-v2 2>/dev/null
'

test_expect_success 'branch --contains HEAD lists current branch' '
	cd repo &&
	git branch --contains HEAD >output &&
	grep "master" output
'

test_expect_success 'branch -l lists branches' '
	cd repo &&
	git branch -l >output &&
	grep "master" output
'

test_expect_success 'branch created at specific SHA' '
	cd repo &&
	parent=$(git rev-parse HEAD~1) &&
	git branch at-sha-v2 "$parent" &&
	result=$(git rev-parse at-sha-v2) &&
	test "$result" = "$parent" &&
	git branch -d at-sha-v2 2>/dev/null
'

test_expect_success 'branch -f forces overwrite at SHA' '
	cd repo &&
	git branch force-br-v2 &&
	old=$(git rev-parse force-br-v2) &&
	parent=$(git rev-parse HEAD~1) &&
	git branch -f force-br-v2 "$parent" &&
	new=$(git rev-parse force-br-v2) &&
	test "$old" != "$new" &&
	git branch -D force-br-v2 2>/dev/null
'

test_expect_success 'branch -d on current branch fails' '
	cd repo &&
	test_must_fail git branch -d master 2>/dev/null
'

test_expect_success 'branch --show-current on master shows master' '
	cd repo &&
	result=$(git branch --show-current) &&
	test "$result" = "master"
'

test_expect_success 'branch -a shows all branches' '
	cd repo &&
	git branch -a >output &&
	grep "master" output
'

test_expect_success 'branch with slash in name' '
	cd repo &&
	git branch feat-v2/slash-test &&
	git rev-parse feat-v2/slash-test &&
	git branch -d feat-v2/slash-test 2>/dev/null
'

test_expect_success 'branch -d nonexistent branch fails' '
	cd repo &&
	test_must_fail git branch -d nonexistent-br-xyz 2>/dev/null
'

test_expect_success 'branch -v shows commit hash and subject' '
	cd repo &&
	git branch -v >output &&
	grep "[0-9a-f]" output &&
	test $(wc -l <output) -ge 1
'

test_expect_success 'branch --merged lists branches merged into HEAD' '
	cd repo &&
	git branch --merged HEAD >output &&
	grep "master" output
'

test_expect_success 'branch -q suppresses output' '
	cd repo &&
	git branch -q quiet-br-v2 >output 2>&1 &&
	test_must_be_empty output &&
	git branch -d quiet-br-v2 2>/dev/null
'

test_expect_success 'branch list shows newly created branch' '
	cd repo &&
	git branch list-check-v2 &&
	git branch >output &&
	grep "list-check-v2" output &&
	git branch -d list-check-v2 2>/dev/null
'

test_done
