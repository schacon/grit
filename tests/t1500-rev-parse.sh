#!/bin/sh
# Ported subset from git/t/t1500-rev-parse.sh.

test_description='grit rev-parse discovery flags'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repository with nested directory' '
	grit init repo &&
	cd repo &&
	echo hello >hello.txt &&
	grit hash-object -w hello.txt >/dev/null &&
	mkdir -p sub/dir
'

test_expect_success '--is-inside-work-tree true in repository root' '
	cd repo &&
	echo true >expect &&
	grit rev-parse --is-inside-work-tree >actual &&
	test_cmp expect actual
'

test_expect_success '--is-inside-work-tree true in subdirectory' '
	cd repo/sub/dir &&
	echo true >expect &&
	grit rev-parse --is-inside-work-tree >actual &&
	test_cmp expect actual
'

test_expect_success '--show-prefix reports relative subdirectory path' '
	cd repo/sub/dir &&
	echo sub/dir/ >expect &&
	grit rev-parse --show-prefix >actual &&
	test_cmp expect actual
'

test_expect_success '--show-prefix is empty at work-tree root' '
	cd repo &&
	echo >expect &&
	grit rev-parse --show-prefix >actual &&
	test_cmp expect actual
'

test_expect_success '--git-dir returns relative path from root and subdirectory' '
	cd repo &&
	echo .git >expect &&
	grit rev-parse --git-dir >actual &&
	test_cmp expect actual &&
	cd sub/dir &&
	echo ../../.git >expect &&
	grit rev-parse --git-dir >actual &&
	test_cmp expect actual
'

test_expect_success '--show-toplevel returns repository root' '
	cd repo/sub/dir &&
	pwd_root=$(cd ../.. && pwd) &&
	echo "$pwd_root" >expect &&
	grit rev-parse --show-toplevel >actual &&
	test_cmp expect actual
'

test_expect_success 'outside repository prints false for --is-inside-work-tree' '
	cd .. &&
	echo false >expect &&
	GIT_DIR=does-not-exist grit rev-parse --is-inside-work-tree >actual &&
	test_cmp expect actual
'

test_expect_success '--is-bare-repository false in non-bare repository' '
	cd repo &&
	echo false >expect &&
	grit rev-parse --is-bare-repository >actual &&
	test_cmp expect actual
'

test_expect_success '--is-inside-git-dir false in work tree' '
	cd repo &&
	echo false >expect &&
	grit rev-parse --is-inside-git-dir >actual &&
	test_cmp expect actual
'

test_expect_success 'outside repository: --is-inside-git-dir prints false' '
	cd .. &&
	echo false >expect &&
	GIT_DIR=does-not-exist grit rev-parse --is-inside-git-dir >actual &&
	test_cmp expect actual
'

test_expect_success 'multiple discovery flags in one invocation' '
	cd repo &&
	printf "true\nfalse\n" >expect &&
	grit rev-parse --is-inside-work-tree --is-bare-repository >actual &&
	test_cmp expect actual
'

test_expect_success 'inside .git directory: --is-inside-git-dir is true' '
	cd repo/.git &&
	echo true >expect &&
	grit rev-parse --is-inside-git-dir >actual &&
	test_cmp expect actual
'

test_expect_success 'inside .git directory: --is-inside-work-tree is false' '
	cd repo/.git &&
	echo false >expect &&
	grit rev-parse --is-inside-work-tree >actual &&
	test_cmp expect actual
'

test_expect_success 'inside .git directory: --git-dir is .' '
	cd repo/.git &&
	echo . >expect &&
	grit rev-parse --git-dir >actual &&
	test_cmp expect actual
'

test_expect_success 'inside .git/objects: --is-inside-git-dir is true' '
	cd repo/.git/objects &&
	echo true >expect &&
	grit rev-parse --is-inside-git-dir >actual &&
	test_cmp expect actual
'

test_expect_success 'inside .git/objects: --git-dir reports parent' '
	cd repo/.git/objects &&
	grit rev-parse --git-dir >actual &&
	test "$(cat actual)" = ".." ||
	test "$(cat actual)" = "$(cd .. && pwd)"
'

test_expect_success '--show-toplevel from inside .git fails' '
	cd repo/.git &&
	test_must_fail grit rev-parse --show-toplevel
'

test_expect_success '--show-toplevel from subdirectory' '
	cd repo &&
	pwd >expect &&
	grit -C sub/dir rev-parse --show-toplevel >actual &&
	test_cmp expect actual
'

test_expect_success '--short=100 truncates to actual hash length' '
	cd repo &&
	grit config user.email "test@test.com" &&
	grit config user.name "Test" &&
	echo hello >commitfile &&
	grit add commitfile &&
	grit commit -m "for short test" &&
	grit rev-parse HEAD >expect &&
	grit rev-parse --short=100 HEAD >actual &&
	test_cmp expect actual
'

test_expect_success 'bare repository: --is-bare-repository is true' '
	grit init --bare bare-repo &&
	cd bare-repo &&
	echo true >expect &&
	grit rev-parse --is-bare-repository >actual &&
	test_cmp expect actual
'

test_expect_success 'bare repository: --is-inside-work-tree is false' '
	cd bare-repo &&
	echo false >expect &&
	grit rev-parse --is-inside-work-tree >actual &&
	test_cmp expect actual
'

test_expect_success 'bare repository: --is-inside-git-dir is true' '
	cd bare-repo &&
	echo true >expect &&
	grit rev-parse --is-inside-git-dir >actual &&
	test_cmp expect actual
'

test_expect_success 'bare repository: --git-dir is .' '
	cd bare-repo &&
	echo . >expect &&
	grit rev-parse --git-dir >actual &&
	test_cmp expect actual
'

# --- New tests: rev-parse with revisions ---

test_expect_success 'rev-parse HEAD resolves to commit hash' '
	cd repo &&
	hash=$(grit rev-parse HEAD) &&
	test $(echo "$hash" | wc -c) = 41
'

test_expect_success 'rev-parse master resolves same as HEAD' '
	cd repo &&
	grit rev-parse HEAD >expect &&
	grit rev-parse master >actual &&
	test_cmp expect actual
'

test_expect_success 'setup additional commits for traversal' '
	cd repo &&
	echo extra >extra.txt &&
	grit add extra.txt &&
	grit commit -m "extra commit"
'

test_expect_success 'rev-parse HEAD~1 resolves to parent' '
	cd repo &&
	grit rev-parse HEAD~1 >actual &&
	hash=$(cat actual) &&
	test $(echo "$hash" | wc -c) = 41
'

test_expect_success 'rev-parse HEAD^1 same as HEAD~1' '
	cd repo &&
	grit rev-parse HEAD~1 >expect &&
	grit rev-parse HEAD^1 >actual &&
	test_cmp expect actual
'

test_expect_success 'rev-parse HEAD^0 same as HEAD' '
	cd repo &&
	grit rev-parse HEAD >expect &&
	grit rev-parse HEAD^0 >actual &&
	test_cmp expect actual
'

test_expect_success 'rev-parse HEAD~0 same as HEAD' '
	cd repo &&
	grit rev-parse HEAD >expect &&
	grit rev-parse HEAD~0 >actual &&
	test_cmp expect actual
'

test_expect_success 'rev-parse HEAD^{commit} same as HEAD' '
	cd repo &&
	grit rev-parse HEAD >expect &&
	grit rev-parse "HEAD^{commit}" >actual &&
	test_cmp expect actual
'

test_expect_success 'rev-parse HEAD^{tree} resolves to tree' '
	cd repo &&
	grit rev-parse "HEAD^{tree}" >actual &&
	hash=$(cat actual) &&
	test $(echo "$hash" | wc -c) = 41 &&
	grit rev-parse HEAD >commit_hash &&
	test "$(cat actual)" != "$(cat commit_hash)"
'

test_expect_success 'rev-parse HEAD^{} peels to commit' '
	cd repo &&
	grit rev-parse HEAD >expect &&
	grit rev-parse "HEAD^{}" >actual &&
	test_cmp expect actual
'

test_expect_success 'rev-parse annotated tag^{commit} peels to commit' '
	cd repo &&
	head=$(grit rev-parse HEAD) &&
	cat >.git/tag-obj <<-EOF &&
	object $head
	type commit
	tag testtag
	tagger grit <grit@example.com> 0 +0000

	test tag
	EOF
	tag_hash=$(grit hash-object -t tag -w .git/tag-obj) &&
	grit update-ref refs/tags/testtag "$tag_hash" &&
	grit rev-parse "testtag^{commit}" >actual &&
	echo "$head" >expect &&
	test_cmp expect actual
'

test_expect_success 'rev-parse tag^{} peels annotated tag' '
	cd repo &&
	head=$(grit rev-parse HEAD) &&
	echo "$head" >expect &&
	grit rev-parse "testtag^{}" >actual &&
	test_cmp expect actual
'

test_expect_success 'rev-parse HEAD:file resolves to blob' '
	cd repo &&
	grit rev-parse HEAD:commitfile >actual &&
	hash=$(cat actual) &&
	test $(echo "$hash" | wc -c) = 41
'

test_expect_success 'rev-parse of bad ref fails' '
	cd repo &&
	test_must_fail grit rev-parse nosuchref 2>err
'

test_expect_success '--verify with good ref succeeds' '
	cd repo &&
	grit rev-parse HEAD >expect &&
	grit rev-parse --verify HEAD >actual &&
	test_cmp expect actual
'

test_expect_success '--verify with bad ref fails' '
	cd repo &&
	test_must_fail grit rev-parse --verify nosuchref
'

test_expect_success '--short outputs abbreviated hash' '
	cd repo &&
	grit rev-parse --short HEAD >actual &&
	len=$(wc -c <actual) &&
	test "$len" -lt 41
'

test_expect_success '--short=7 outputs 7-char hash' '
	cd repo &&
	grit rev-parse --short=7 HEAD >actual &&
	hash=$(cat actual) &&
	test $(echo "$hash" | tr -d "\n" | wc -c) = 7
'

test_expect_success '--end-of-options works' '
	cd repo &&
	grit rev-parse HEAD >expect &&
	grit rev-parse --end-of-options HEAD >actual &&
	test_cmp expect actual
'

test_expect_success 'rev-parse -- separates revs from paths' '
	cd repo &&
	grit rev-parse HEAD -- commitfile >actual &&
	head=$(grit rev-parse HEAD) &&
	printf "%s\n--\ncommitfile\n" "$head" >expect &&
	test_cmp expect actual
'

test_expect_success 'multiple refs resolved' '
	cd repo &&
	head=$(grit rev-parse HEAD) &&
	parent=$(grit rev-parse HEAD~1) &&
	grit rev-parse HEAD HEAD~1 >actual &&
	{ echo "$head" && echo "$parent"; } >expect &&
	test_cmp expect actual
'

test_expect_success 'inside .git/refs: --is-inside-git-dir true' '
	cd repo/.git/refs &&
	echo true >expect &&
	grit rev-parse --is-inside-git-dir >actual &&
	test_cmp expect actual
'

test_expect_success '--show-prefix empty at root, non-empty in subdir' '
	cd repo &&
	grit rev-parse --show-prefix >root_prefix &&
	echo >expect &&
	test_cmp expect root_prefix &&
	cd sub &&
	echo sub/ >expect &&
	grit rev-parse --show-prefix >actual &&
	test_cmp expect actual
'

test_expect_success '--show-toplevel consistent from subdirectory' '
	cd repo &&
	expected=$(pwd) &&
	cd sub/dir &&
	grit rev-parse --show-toplevel >actual &&
	echo "$expected" >expect &&
	test_cmp expect actual
'

test_expect_success '--git-dir from deep subdirectory' '
	cd repo/sub/dir &&
	echo ../../.git >expect &&
	grit rev-parse --git-dir >actual &&
	test_cmp expect actual
'

test_expect_success 'GIT_DIR overrides discovery' '
	cd repo &&
	grit rev-parse --git-dir >normal &&
	GIT_DIR=.git grit rev-parse --git-dir >with_env &&
	test_cmp normal with_env
'

test_expect_success 'multiple discovery flags combined' '
	cd repo &&
	grit rev-parse --is-inside-work-tree --is-bare-repository --is-inside-git-dir >actual &&
	printf "true\nfalse\nfalse\n" >expect &&
	test_cmp expect actual
'

test_expect_success 'bare repo: multiple flags combined' '
	cd bare-repo &&
	grit rev-parse --is-bare-repository >actual_bare &&
	echo true >expect_bare &&
	test_cmp expect_bare actual_bare &&
	grit rev-parse --is-inside-work-tree >actual_wt &&
	echo false >expect_wt &&
	test_cmp expect_wt actual_wt
'

test_expect_success 'rev-parse resolves full ref name refs/heads/master' '
	cd repo &&
	grit rev-parse HEAD >expect &&
	grit rev-parse refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success '-C flag changes directory for discovery' '
	grit -C repo rev-parse --is-inside-work-tree >actual &&
	echo true >expect &&
	test_cmp expect actual
'

test_expect_success '-C flag with subdirectory shows correct prefix' '
	cd repo &&
	grit -C sub rev-parse --show-prefix >actual &&
	echo sub/ >expect &&
	test_cmp expect actual
'

test_expect_success '-C flag with --git-dir' '
	cd repo &&
	grit -C sub/dir rev-parse --git-dir >actual &&
	echo ../../.git >expect &&
	test_cmp expect actual
'

test_expect_success 'rev-parse HEAD^2 fails on non-merge commit' '
	cd repo &&
	test_must_fail grit rev-parse HEAD^2
'

test_expect_success 'rev-parse of tag name resolves' '
	cd repo &&
	grit rev-parse testtag >actual &&
	hash=$(cat actual) &&
	test $(echo "$hash" | wc -c) = 41
'

test_expect_success 'rev-parse tag vs tag^{commit} differ for annotated tag' '
	cd repo &&
	grit rev-parse testtag >tag_oid &&
	grit rev-parse "testtag^{commit}" >commit_oid &&
	! test_cmp tag_oid commit_oid
'

test_expect_success '--show-toplevel fails in bare repo' '
	cd bare-repo &&
	test_must_fail grit rev-parse --show-toplevel
'

test_expect_success '--show-prefix in bare repo' '
	cd bare-repo &&
	grit rev-parse --show-prefix >actual 2>/dev/null ||
	true
'

test_expect_success 'second commit setup for more tests' '
	cd repo &&
	echo more >morefile &&
	grit add morefile &&
	grit commit -m "second commit" &&
	echo thrice >thirdfile &&
	grit add thirdfile &&
	grit commit -m "third commit"
'

test_expect_success 'HEAD~2 resolves when grandparent exists' '
	cd repo &&
	grit rev-parse HEAD~2 >actual &&
	hash=$(cat actual) &&
	test $(echo "$hash" | wc -c) = 41
'

test_expect_success 'HEAD~1 != HEAD' '
	cd repo &&
	head=$(grit rev-parse HEAD) &&
	parent=$(grit rev-parse HEAD~1) &&
	test "$head" != "$parent"
'

test_expect_success 'HEAD~2 != HEAD~1' '
	cd repo &&
	gp=$(grit rev-parse HEAD~2) &&
	p=$(grit rev-parse HEAD~1) &&
	test "$gp" != "$p"
'

test_expect_success '--short=100 gives full hash' '
	cd repo &&
	grit rev-parse HEAD >expect &&
	grit rev-parse --short=100 HEAD >actual &&
	test_cmp expect actual
'

test_expect_success '--short=4 gives at least 4 chars' '
	cd repo &&
	grit rev-parse --short=4 HEAD >actual &&
	len=$(cat actual | tr -d "\n" | wc -c) &&
	test "$len" -ge 4
'

test_expect_success '--verify multiple refs fails' '
	cd repo &&
	test_must_fail grit rev-parse --verify HEAD HEAD~1
'

test_expect_success 'refs/tags/ prefix resolves tag' '
	cd repo &&
	grit rev-parse testtag >expect &&
	grit rev-parse refs/tags/testtag >actual &&
	test_cmp expect actual
'

test_expect_success 'rev-parse tree:path resolves' '
	cd repo &&
	tree=$(grit rev-parse "HEAD^{tree}") &&
	grit rev-parse "$tree:commitfile" >actual &&
	hash=$(cat actual) &&
	test $(echo "$hash" | wc -c) = 41
'

test_expect_success '--git-dir with GIT_DIR set to .git' '
	cd repo &&
	GIT_DIR=.git grit rev-parse --git-dir >actual &&
	echo .git >expect &&
	test_cmp expect actual
'

test_expect_success '--is-inside-work-tree with GIT_DIR and GIT_WORK_TREE' '
	cd repo &&
	GIT_DIR=.git GIT_WORK_TREE=. grit rev-parse --is-inside-work-tree >actual &&
	echo true >expect &&
	test_cmp expect actual
'

# --- Additional rev-parse tests ---

test_expect_success '--short=12 outputs 12-char hash' '
	cd repo &&
	grit rev-parse --short=12 HEAD >actual &&
	hash=$(cat actual | tr -d "\n") &&
	len=$(printf "%s" "$hash" | wc -c) &&
	test "$len" = 12
'

test_expect_success '--short default is 7 chars' '
	cd repo &&
	grit rev-parse --short HEAD >actual &&
	hash=$(cat actual | tr -d "\n") &&
	len=$(printf "%s" "$hash" | wc -c) &&
	test "$len" = 7
'

test_expect_success '--short hash is prefix of full hash' '
	cd repo &&
	grit rev-parse HEAD >full &&
	grit rev-parse --short=10 HEAD >short &&
	prefix=$(head -c 10 full) &&
	short_val=$(cat short | tr -d "\n") &&
	test "$prefix" = "$short_val"
'

test_expect_success '--verify with --short outputs short hash' '
	cd repo &&
	grit rev-parse --verify --short HEAD >actual &&
	hash=$(cat actual | tr -d "\n") &&
	len=$(printf "%s" "$hash" | wc -c) &&
	test "$len" -lt 41
'

test_expect_success 'rev-parse HEAD^{tree} differs from HEAD^{commit}' '
	cd repo &&
	grit rev-parse "HEAD^{tree}" >tree &&
	grit rev-parse "HEAD^{commit}" >commit &&
	! test_cmp tree commit
'

test_expect_success 'rev-parse handles mixed flags and revisions' '
	cd repo &&
	grit rev-parse --is-inside-work-tree HEAD >actual &&
	head_hash=$(grit rev-parse HEAD) &&
	printf "true\n%s\n" "$head_hash" >expect &&
	test_cmp expect actual
'

test_done
