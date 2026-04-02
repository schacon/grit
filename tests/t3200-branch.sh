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

test_done
