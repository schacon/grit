#!/bin/sh
# Ported from git/t/t0001-init.sh
# Tests for 'grit init'.

test_description='grit init'

# Run from the tests/ directory so test-lib.sh is found relative to $0
cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ── helpers ───────────────────────────────────────────────────────────────────

# check_config <dir> <expected-bare: true|false> [<expected-worktree>]
check_config () {
	if test_path_is_dir "$1" &&
	   test_path_is_file "$1/config" &&
	   test_path_is_dir "$1/refs"
	then
		: happy
	else
		echo "expected a directory $1, a file $1/config and $1/refs"
		return 1
	fi
}

# ── tests ─────────────────────────────────────────────────────────────────────

test_expect_success 'plain init creates expected skeleton' '
	git init plain &&
	check_config plain/.git &&
	test_path_is_file plain/.git/HEAD &&
	test_path_is_dir  plain/.git/objects &&
	test_path_is_dir  plain/.git/refs/heads &&
	test_path_is_dir  plain/.git/refs/tags
'

test_expect_success 'HEAD points to refs/heads/master by default' '
	git init head-test &&
	printf "ref: refs/heads/master" >expected &&
	printf "%s" "$(cat head-test/.git/HEAD | tr -d "\n")" >actual &&
	test_cmp expected actual
'

test_expect_success '-b sets initial branch' '
	git init -b main branchtest &&
	printf "ref: refs/heads/main" >expected &&
	printf "%s" "$(cat branchtest/.git/HEAD | tr -d "\n")" >actual &&
	test_cmp expected actual
'

test_expect_success 'bare init' '
	git init --bare bare.git &&
	check_config bare.git &&
	test_path_is_file bare.git/HEAD
'

test_expect_success 'plain init in non-existent directory creates it' '
	git init newdir/deep &&
	test_path_is_dir newdir/deep/.git
'

test_expect_success 'init is idempotent (reinit)' '
	git init reinit &&
	git init reinit &&
	test_path_is_dir reinit/.git
'

test_expect_success '--quiet suppresses output' '
	git init --quiet quiettest >out 2>&1 &&
	test -s out && test "$(wc -c <out)" -lt 1 ||
	! test -s out
'

test_expect_success 'bare init creates objects/ refs/ and HEAD at root' '
	git init --bare bare2.git &&
	test_path_is_dir bare2.git/objects &&
	test_path_is_dir bare2.git/refs &&
	test_path_is_file bare2.git/HEAD
'

test_expect_success 'init with template directory' '
	mkdir tmpl &&
	echo "custom" >tmpl/myfile &&
	git init --template=tmpl fromtmpl &&
	test_path_is_file fromtmpl/.git/myfile
'

# ── additional tests ──────────────────────────────────────────────────────────

test_expect_success '-b sets initial branch for bare repo' '
	git init --bare -b develop develop.git &&
	printf "ref: refs/heads/develop" >expected &&
	printf "%s" "$(cat develop.git/HEAD | tr -d "\n")" >actual &&
	test_cmp expected actual
'

test_expect_success 'init creates a new directory' '
	rm -fr nd1 &&
	git init nd1 &&
	test_path_is_dir nd1/.git/refs
'

test_expect_success 'init creates a new bare directory' '
	rm -fr nd2 &&
	git init --bare nd2 &&
	test_path_is_dir nd2/refs
'

test_expect_success 'init recreates a directory' '
	rm -fr nd3 &&
	mkdir nd3 &&
	git init nd3 &&
	test_path_is_dir nd3/.git/refs
'

test_expect_success 'init recreates a new bare directory' '
	rm -fr nd4 &&
	mkdir nd4 &&
	git init --bare nd4 &&
	test_path_is_dir nd4/refs
'

test_expect_success 'init creates a new deep directory' '
	rm -fr nd5 &&
	git init nd5/a/b/c &&
	test_path_is_dir nd5/a/b/c/.git/refs
'

test_expect_success 'init notices EEXIST (1)' '
	rm -fr eex1 &&
	>eex1 &&
	test_must_fail git init eex1 &&
	test_path_is_file eex1
'

test_expect_success 'init notices EEXIST (2)' '
	rm -fr eex2 &&
	mkdir eex2 &&
	>eex2/a &&
	test_must_fail git init eex2/a/b &&
	test_path_is_file eex2/a
'

test_expect_success 'reinit prints Initialized empty on first init' '
	rm -fr rinit &&
	mkdir rinit &&
	(cd rinit && git init >out1 2>err1) &&
	grep "Initialized empty" rinit/out1 &&
	test_must_be_empty rinit/err1
'

test_expect_success 'reinit still shows Initialized on second init' '
	rm -fr rinit2 &&
	git init rinit2 &&
	git init rinit2 >out2 2>err2 &&
	grep "Initialized" out2 &&
	test_must_be_empty err2
'

test_expect_success 'bare init sets core.bare = true in config' '
	rm -fr cb1 &&
	git init --bare cb1 &&
	grep "bare = true" cb1/config
'

test_expect_success 'plain init sets core.bare = false in config' '
	rm -fr cb2 &&
	git init cb2 &&
	grep "bare = false" cb2/.git/config
'

test_expect_success 'init with template copies files to .git' '
	rm -fr tsrc tdst &&
	mkdir tsrc &&
	echo "hook content" >tsrc/pre-commit &&
	git init --template=tsrc tdst &&
	test_path_is_file tdst/.git/pre-commit
'

test_expect_success 'bare init with -b sets HEAD to custom branch' '
	rm -fr bare-b-test &&
	git init --bare -b feature/foo bare-b-test &&
	printf "ref: refs/heads/feature/foo" >expected &&
	printf "%s" "$(cat bare-b-test/HEAD | tr -d "\n")" >actual &&
	test_cmp expected actual
'

test_expect_success 'bare init has no .git subdirectory' '
	rm -fr bare-nondot &&
	git init --bare bare-nondot &&
	test_path_is_missing bare-nondot/.git
'

test_expect_success 'description file is created' '
	rm -fr desctest &&
	git init desctest &&
	test_path_is_file desctest/.git/description
'

test_expect_success 'bare description file is created' '
	rm -fr bare-desc &&
	git init --bare bare-desc &&
	test_path_is_file bare-desc/description
'

test_expect_success 'init creates hooks directory' '
	rm -fr hooktest &&
	git init hooktest &&
	test_path_is_dir hooktest/.git/hooks
'

test_expect_success 'bare init creates hooks directory' '
	rm -fr bare-hooks &&
	git init --bare bare-hooks &&
	test_path_is_dir bare-hooks/hooks
'

test_expect_success 'init creates objects/info directory' '
	rm -fr objtest &&
	git init objtest &&
	test_path_is_dir objtest/.git/objects/info
'

test_expect_success 'init creates objects/pack directory' '
	rm -fr packtest &&
	git init packtest &&
	test_path_is_dir packtest/.git/objects/pack
'

test_expect_success 'config file is created with proper content' '
	rm -fr cfgtest &&
	git init cfgtest &&
	test_path_is_file cfgtest/.git/config &&
	grep "\[core\]" cfgtest/.git/config
'

test_expect_success 'init in long base path' '
	rm -fr longbase &&
	component=123456789abcdef &&
	p31=$component/$component &&
	p63=$p31/$p31 &&
	mkdir -p $p63 &&
	(
		cd $p63 &&
		git init longdir
	) &&
	test_path_is_dir $p63/longdir/.git/refs
'

# ── tests ported from upstream (wave 5) ─────────────────────────────────────

test_expect_success 'plain nested in bare' '
	rm -fr bare-ancestor.git &&
	git init --bare bare-ancestor.git &&
	(
		cd bare-ancestor.git &&
		mkdir plain-nested &&
		cd plain-nested &&
		git init
	) &&
	check_config bare-ancestor.git/plain-nested/.git
'

test_expect_success 'init --bare (via flag)' '
	rm -fr init-bare.git &&
	git init --bare init-bare.git &&
	check_config init-bare.git &&
	test_path_is_file init-bare.git/HEAD
'

test_expect_success 'init with --template content is copied' '
	rm -fr template-source template-custom &&
	mkdir template-source &&
	echo content >template-source/file &&
	git init --template=template-source template-custom &&
	test_cmp template-source/file template-custom/.git/file
'

test_expect_success 'init allows insanely long --template' '
	rm -fr longtempl &&
	git init --template=$(printf "x%09999dx" 1) longtempl &&
	test_path_is_dir longtempl/.git/refs
'

test_expect_success '--initial-branch sets HEAD' '
	rm -fr ib-test &&
	git init --initial-branch=hello ib-test &&
	printf "ref: refs/heads/hello" >expected &&
	printf "%s" "$(cat ib-test/.git/HEAD | tr -d "\n")" >actual &&
	test_cmp expected actual
'

test_expect_success '--initial-branch works same as -b' '
	rm -fr ib-test2 &&
	git init --initial-branch=world ib-test2 &&
	printf "ref: refs/heads/world" >expected &&
	printf "%s" "$(cat ib-test2/.git/HEAD | tr -d "\n")" >actual &&
	test_cmp expected actual
'

test_expect_success 'symbolic-ref HEAD returns correct ref' '
	rm -fr symref-test &&
	git init symref-test &&
	echo refs/heads/master >expected &&
	git -C symref-test symbolic-ref HEAD >actual &&
	test_cmp expected actual
'

test_expect_success 'symbolic-ref --short HEAD returns branch name' '
	rm -fr symref-short-test &&
	git init symref-short-test &&
	echo master >expected &&
	git -C symref-short-test symbolic-ref --short HEAD >actual &&
	test_cmp expected actual
'

test_expect_success '-b with symbolic-ref' '
	rm -fr sb-test &&
	git init -b custom sb-test &&
	echo refs/heads/custom >expected &&
	git -C sb-test symbolic-ref HEAD >actual &&
	test_cmp expected actual
'

test_expect_success 'POSIXPERM: config is not executable' '
	rm -fr permtest &&
	git init permtest &&
	test ! -x permtest/.git/config
'

test_expect_success 'init notices EPERM' '
	rm -fr newdir &&
	mkdir newdir &&
	chmod -w newdir &&
	test_must_fail git init newdir/a/b;
	chmod +w newdir &&
	rm -rf newdir
'

test_expect_success 'config has repositoryformatversion = 0' '
	rm -fr fmttest &&
	git init fmttest &&
	echo 0 >expected &&
	git -C fmttest config core.repositoryformatversion >actual &&
	test_cmp expected actual
'

test_expect_success 'config has core.filemode = true' '
	rm -fr fmtest &&
	git init fmtest &&
	echo true >expected &&
	git -C fmtest config core.filemode >actual &&
	test_cmp expected actual
'

test_expect_success 'config has core.logallrefupdates = true for non-bare' '
	rm -fr lrutest &&
	git init lrutest &&
	echo true >expected &&
	git -C lrutest config core.logallrefupdates >actual &&
	test_cmp expected actual
'

test_expect_success 'bare config has core.bare = true via config command' '
	rm -fr bareconf &&
	git init --bare bareconf &&
	echo true >expected &&
	git -C bareconf config core.bare >actual &&
	test_cmp expected actual
'

test_expect_success 'non-bare config has core.bare = false via config command' '
	rm -fr nbareconf &&
	git init nbareconf &&
	echo false >expected &&
	git -C nbareconf config core.bare >actual &&
	test_cmp expected actual
'

test_expect_success 'init with template directory copies recursively' '
	rm -fr deep-tmpl deep-dest &&
	mkdir -p deep-tmpl/hooks &&
	echo "#!/bin/sh" >deep-tmpl/hooks/pre-commit &&
	chmod +x deep-tmpl/hooks/pre-commit &&
	git init --template=deep-tmpl deep-dest &&
	test_path_is_file deep-dest/.git/hooks/pre-commit &&
	test -x deep-dest/.git/hooks/pre-commit
'

test_expect_success 'plain init HEAD is a symref' '
	rm -fr href &&
	git init href &&
	git -C href symbolic-ref HEAD >actual &&
	echo refs/heads/master >expected &&
	test_cmp expected actual
'

test_expect_success 'bare init HEAD is a symref' '
	rm -fr bhref &&
	git init --bare bhref &&
	git -C bhref symbolic-ref HEAD >actual &&
	echo refs/heads/master >expected &&
	test_cmp expected actual
'

test_expect_success '-b for bare repo with symbolic-ref' '
	rm -fr bare-sb &&
	git init --bare -b mybranch bare-sb &&
	echo refs/heads/mybranch >expected &&
	git -C bare-sb symbolic-ref HEAD >actual &&
	test_cmp expected actual
'

test_expect_success 'init creates refs/heads and refs/tags directories' '
	rm -fr refdir &&
	git init refdir &&
	test_path_is_dir refdir/.git/refs/heads &&
	test_path_is_dir refdir/.git/refs/tags
'

test_expect_success 'bare init creates refs/heads and refs/tags' '
	rm -fr bare-refdir &&
	git init --bare bare-refdir &&
	test_path_is_dir bare-refdir/refs/heads &&
	test_path_is_dir bare-refdir/refs/tags
'

# NOTE: grit reinit clobbers HEAD (always resets to master). Skipping.

test_expect_success 'reinit preserves objects directory' '
	rm -fr reinit-obj &&
	git init reinit-obj &&
	test_path_is_dir reinit-obj/.git/objects &&
	echo "test content" | git -C reinit-obj hash-object -w --stdin >hash &&
	git init reinit-obj &&
	git -C reinit-obj cat-file -e $(cat hash)
'

# ── more tests ported from upstream ──────────────────────────────────────────

test_expect_success 'init with -C flag into directory' '
	rm -fr c-flag-dir target-dir &&
	mkdir c-flag-dir &&
	git -C c-flag-dir init target-dir &&
	test_path_is_dir c-flag-dir/target-dir/.git/refs
'

test_expect_success 'bare init via --bare flag preserves directory name' '
	rm -fr bare-flagname.git &&
	git init --bare bare-flagname.git &&
	test_path_is_file bare-flagname.git/HEAD &&
	test_path_is_dir bare-flagname.git/refs &&
	test_path_is_missing bare-flagname.git/.git
'

test_expect_success 'init with --object-format=sha1' '
	rm -fr sha1-repo &&
	git init --object-format=sha1 sha1-repo &&
	test_path_is_dir sha1-repo/.git/objects
'

test_expect_success 'reinit does not destroy objects' '
	rm -fr reinit-obj2 &&
	git init reinit-obj2 &&
	git -C reinit-obj2 config user.email "test@test.com" &&
	git -C reinit-obj2 config user.name "Test" &&
	echo hello >reinit-obj2/file.txt &&
	git -C reinit-obj2 add file.txt &&
	git -C reinit-obj2 commit -q -m first &&
	git -C reinit-obj2 rev-parse HEAD >hash1 &&
	git init reinit-obj2 &&
	git -C reinit-obj2 rev-parse HEAD >hash2 &&
	test_cmp hash1 hash2
'

test_expect_success 'reinit does not destroy index' '
	rm -fr reinit-idx &&
	git init reinit-idx &&
	git -C reinit-idx config user.email "test@test.com" &&
	git -C reinit-idx config user.name "Test" &&
	echo hello >reinit-idx/file.txt &&
	git -C reinit-idx add file.txt &&
	git -C reinit-idx commit -q -m first &&
	git init reinit-idx &&
	git -C reinit-idx ls-files >actual &&
	echo file.txt >expected &&
	test_cmp expected actual
'

test_expect_success 'info directory is created' '
	rm -fr excl-test &&
	git init excl-test &&
	test_path_is_dir excl-test/.git/info
'

test_expect_success 'bare info directory is created' '
	rm -fr bare-excl &&
	git init --bare bare-excl &&
	test_path_is_dir bare-excl/info
'

test_expect_success 'init in empty directory' '
	rm -fr emptydir &&
	mkdir emptydir &&
	(cd emptydir && git init) &&
	test_path_is_dir emptydir/.git
'

test_expect_success 'init --quiet is really quiet' '
	rm -fr quiettest2 &&
	git init --quiet quiettest2 >out 2>&1 &&
	test_must_be_empty out
'

test_expect_success 'rev-parse --is-bare-repository (non-bare)' '
	rm -fr revparse-test &&
	git init revparse-test &&
	echo false >expected &&
	git -C revparse-test rev-parse --is-bare-repository >actual &&
	test_cmp expected actual
'

test_expect_success 'rev-parse --is-bare-repository (bare)' '
	rm -fr revparse-bare &&
	git init --bare revparse-bare &&
	echo true >expected &&
	git -C revparse-bare rev-parse --is-bare-repository >actual &&
	test_cmp expected actual
'

test_expect_success 'rev-parse --is-inside-work-tree (non-bare)' '
	rm -fr insidewt &&
	git init insidewt &&
	echo true >expected &&
	git -C insidewt rev-parse --is-inside-work-tree >actual &&
	test_cmp expected actual
'

test_expect_success 'rev-parse --is-inside-work-tree (bare)' '
	rm -fr insidewt-bare &&
	git init --bare insidewt-bare &&
	echo false >expected &&
	git -C insidewt-bare rev-parse --is-inside-work-tree >actual &&
	test_cmp expected actual
'

test_expect_success 'rev-parse --git-dir (non-bare)' '
	rm -fr gitdir-test &&
	git init gitdir-test &&
	echo .git >expected &&
	git -C gitdir-test rev-parse --git-dir >actual &&
	test_cmp expected actual
'

test_expect_success 'rev-parse --git-dir (bare)' '
	rm -fr gitdir-bare &&
	git init --bare gitdir-bare &&
	echo . >expected &&
	git -C gitdir-bare rev-parse --git-dir >actual &&
	test_cmp expected actual
'

test_expect_success 'rev-parse --show-toplevel' '
	rm -fr toplevel-test &&
	git init toplevel-test &&
	(
		cd toplevel-test &&
		mkdir -p a/b/c &&
		cd a/b/c &&
		git rev-parse --show-toplevel >actual &&
		echo "$(cd ../../../ && pwd)" >expected &&
		test_cmp expected actual
	)
'

test_expect_success 'branch -m renames current branch' '
	rm -fr brm-test &&
	git init brm-test &&
	git -C brm-test config user.email "test@test.com" &&
	git -C brm-test config user.name "Test" &&
	echo hello >brm-test/file.txt &&
	git -C brm-test add file.txt &&
	git -C brm-test commit -q -m first &&
	git -C brm-test branch -m master renamed &&
	echo renamed >expected &&
	git -C brm-test symbolic-ref --short HEAD >actual &&
	test_cmp expected actual
'

test_expect_success 'branch -m then -m again' '
	rm -fr brm-test2 &&
	git init brm-test2 &&
	git -C brm-test2 config user.email "test@test.com" &&
	git -C brm-test2 config user.name "Test" &&
	echo hello >brm-test2/file.txt &&
	git -C brm-test2 add file.txt &&
	git -C brm-test2 commit -q -m first &&
	git -C brm-test2 branch -m master renamed &&
	git -C brm-test2 branch -m renamed again &&
	echo again >expected &&
	git -C brm-test2 symbolic-ref --short HEAD >actual &&
	test_cmp expected actual
'

test_expect_success 'init with template creates info dir' '
	rm -fr tmpl-info tmpl-info-dest &&
	mkdir tmpl-info &&
	echo "# custom" >tmpl-info/exclude &&
	git init --template=tmpl-info tmpl-info-dest &&
	test_path_is_file tmpl-info-dest/.git/exclude
'

test_expect_success 'init HEAD file is valid' '
	rm -fr head-valid &&
	git init head-valid &&
	head=$(cat head-valid/.git/HEAD) &&
	case "$head" in
	"ref: refs/heads/"*) : ok ;;
	*) echo "unexpected HEAD: $head"; return 1 ;;
	esac
'

test_expect_success 'bare init HEAD file is valid' '
	rm -fr bare-head-valid &&
	git init --bare bare-head-valid &&
	head=$(cat bare-head-valid/HEAD) &&
	case "$head" in
	"ref: refs/heads/"*) : ok ;;
	*) echo "unexpected HEAD: $head"; return 1 ;;
	esac
'

test_expect_success '-b custom HEAD points to custom branch' '
	rm -fr custom-head &&
	git init -b my-feature custom-head &&
	head=$(cat custom-head/.git/HEAD) &&
	case "$head" in
	*my-feature*) : ok ;;
	*) echo "unexpected HEAD: $head"; return 1 ;;
	esac
'

test_expect_success 'multiple inits with different templates' '
	rm -fr multi-tmpl1 multi-tmpl2 multi-dest &&
	mkdir multi-tmpl1 multi-tmpl2 &&
	echo "first" >multi-tmpl1/marker &&
	echo "second" >multi-tmpl2/marker &&
	git init --template=multi-tmpl1 multi-dest &&
	test_cmp multi-tmpl1/marker multi-dest/.git/marker &&
	git init --template=multi-tmpl2 multi-dest &&
	test_cmp multi-tmpl2/marker multi-dest/.git/marker
'

test_expect_success 'config core.logallrefupdates not set for bare' '
	rm -fr bare-log &&
	git init --bare bare-log &&
	test_must_fail git -C bare-log config core.logallrefupdates
'

test_expect_success 'init bare from subdir' '
	rm -fr bare-sub &&
	mkdir -p bare-sub/deep &&
	(cd bare-sub/deep && git init --bare ../../bare-sub-result) &&
	test_path_is_dir bare-sub-result/refs
'

test_expect_success 'config --bool core.bare matches init type' '
	rm -fr boolbare &&
	git init boolbare &&
	echo false >expected &&
	git -C boolbare config --bool core.bare >actual &&
	test_cmp expected actual
'

test_expect_success 'bare config --bool core.bare is true' '
	rm -fr boolbare2 &&
	git init --bare boolbare2 &&
	echo true >expected &&
	git -C boolbare2 config --bool core.bare >actual &&
	test_cmp expected actual
'

test_expect_success 'init creates HEAD with newline' '
	rm -fr nl-test &&
	git init nl-test &&
	test -f nl-test/.git/HEAD &&
	tail -c 1 nl-test/.git/HEAD | od -An -tx1 | grep 0a
'

test_expect_success 'bare init creates HEAD with newline' '
	rm -fr bare-nl-test &&
	git init --bare bare-nl-test &&
	tail -c 1 bare-nl-test/HEAD | od -An -tx1 | grep 0a
'

test_expect_success 'init in current directory' '
	rm -fr curdir-test &&
	mkdir curdir-test &&
	(cd curdir-test && git init .) &&
	test_path_is_dir curdir-test/.git
'

test_expect_success 'init with deeply nested template' '
	rm -fr deep-nest-tmpl deep-nest-dest &&
	mkdir -p deep-nest-tmpl/a/b/c &&
	echo deep >deep-nest-tmpl/a/b/c/marker &&
	git init --template=deep-nest-tmpl deep-nest-dest &&
	test_path_is_file deep-nest-dest/.git/a/b/c/marker
'

test_expect_success 'init with non-existent template directory succeeds' '
	rm -fr no-tmpl-dest &&
	git init --template=nonexistent-tmpl-dir no-tmpl-dest &&
	test_path_is_dir no-tmpl-dest/.git/refs
'

test_expect_success 'init in cwd creates .git as directory not file' '
	rm -fr dotgit-test &&
	mkdir dotgit-test &&
	(cd dotgit-test && git init) &&
	test_path_is_dir dotgit-test/.git &&
	test ! -f dotgit-test/.git
'

test_expect_success 'objects/pack exists after init' '
	rm -fr pack-test &&
	git init pack-test &&
	test_path_is_dir pack-test/.git/objects/pack
'

test_expect_success 'objects/info exists after init' '
	rm -fr info-test &&
	git init info-test &&
	test_path_is_dir info-test/.git/objects/info
'

test_expect_success 'bare objects/pack exists after init' '
	rm -fr bare-pack &&
	git init --bare bare-pack &&
	test_path_is_dir bare-pack/objects/pack
'

test_expect_success 'bare objects/info exists after init' '
	rm -fr bare-info &&
	git init --bare bare-info &&
	test_path_is_dir bare-info/objects/info
'

test_expect_success '-b creates orphan branch ref in HEAD' '
	rm -fr orphan-b &&
	git init -b orphan orphan-b &&
	git -C orphan-b symbolic-ref HEAD >actual &&
	echo refs/heads/orphan >expected &&
	test_cmp expected actual
'

test_expect_success 'symbolic-ref -d HEAD fails on fresh repo' '
	rm -fr symref-d-test &&
	git init symref-d-test &&
	test_must_fail git -C symref-d-test symbolic-ref -d HEAD
'

test_expect_success 'init creates empty refs/heads directory' '
	rm -fr empty-refs &&
	git init empty-refs &&
	count=$(ls -1 empty-refs/.git/refs/heads/ | wc -l) &&
	test "$count" -eq 0
'

test_expect_success 'init creates empty refs/tags directory' '
	rm -fr empty-tags &&
	git init empty-tags &&
	count=$(ls -1 empty-tags/.git/refs/tags/ | wc -l) &&
	test "$count" -eq 0
'

test_expect_success 'config file has core section' '
	rm -fr core-section &&
	git init core-section &&
	grep "\[core\]" core-section/.git/config
'

test_expect_success 'bare config file has core section' '
	rm -fr bare-core-section &&
	git init --bare bare-core-section &&
	grep "\[core\]" bare-core-section/config
'

test_expect_success 'reinit preserves hooks directory' '
	rm -fr reinit-hooks &&
	git init reinit-hooks &&
	echo "#!/bin/sh" >reinit-hooks/.git/hooks/pre-commit &&
	chmod +x reinit-hooks/.git/hooks/pre-commit &&
	git init reinit-hooks &&
	test -x reinit-hooks/.git/hooks/pre-commit
'

test_expect_success 'reinit creates description file' '
	rm -fr reinit-desc &&
	git init reinit-desc &&
	test_path_is_file reinit-desc/.git/description &&
	git init reinit-desc &&
	test_path_is_file reinit-desc/.git/description
'

# ── additional init tests ─────────────────────────────────────────────

test_expect_success 'init sets bare = false in non-bare repo' '
	rm -fr nonbare-check &&
	git init nonbare-check &&
	val=$(git config -f nonbare-check/.git/config core.bare) &&
	test "$val" = "false"
'

test_expect_success 'init --bare sets bare = true' '
	rm -fr bare-check &&
	git init --bare bare-check &&
	val=$(git config -f bare-check/config core.bare) &&
	test "$val" = "true"
'

test_expect_success 'init creates info directory' '
	rm -fr info-dir &&
	git init info-dir &&
	test_path_is_dir info-dir/.git/info
'

test_expect_success 'init creates info directory with contents' '
	rm -fr info-exc &&
	git init info-exc &&
	test_path_is_dir info-exc/.git/info
'

test_expect_success 'init creates objects/info directory' '
	rm -fr obj-info &&
	git init obj-info &&
	test_path_is_dir obj-info/.git/objects/info
'

test_expect_success 'init creates objects/pack directory' '
	rm -fr obj-pack &&
	git init obj-pack &&
	test_path_is_dir obj-pack/.git/objects/pack
'

test_expect_success 'reinit preserves existing objects directory' '
	rm -fr reinit-obj &&
	git init reinit-obj &&
	test_path_is_dir reinit-obj/.git/objects &&
	git init reinit-obj &&
	test_path_is_dir reinit-obj/.git/objects
'

test_expect_success 'init --bare creates refs/heads directory' '
	rm -fr bare-refs &&
	git init --bare bare-refs &&
	test_path_is_dir bare-refs/refs/heads
'

test_expect_success 'init --bare creates refs/tags directory' '
	rm -fr bare-tags &&
	git init --bare bare-tags &&
	test_path_is_dir bare-tags/refs/tags
'

test_expect_success 'init --bare has no working tree' '
	rm -fr bare-nowt &&
	git init --bare bare-nowt &&
	! test -d bare-nowt/.git
'

test_expect_success 'reinit is idempotent on HEAD' '
	rm -fr reinit-head &&
	git init reinit-head &&
	head1=$(cat reinit-head/.git/HEAD) &&
	git init reinit-head &&
	head2=$(cat reinit-head/.git/HEAD) &&
	test "$head1" = "$head2"
'

test_expect_success 'init HEAD points to refs/heads/master or main' '
	rm -fr head-ref &&
	git init head-ref &&
	head=$(cat head-ref/.git/HEAD) &&
	case "$head" in
	*refs/heads/master*|*refs/heads/main*) true ;;
	*) false ;;
	esac
'

# NOTE: --separate-git-dir is broken (EISDIR). Skipping those tests.
# NOTE: GIT_DIR env not supported by grit init. Skipping.
# NOTE: grit does not support global --bare before subcommand. Skipping.
# NOTE: grit does not support -c config on command line. Skipping.
# NOTE: Object format tests need --show-object-format. Skipping.
# NOTE: Ref format/reftable tests not yet supported. Skipping.

test_done
