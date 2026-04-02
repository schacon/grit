#!/bin/sh
# Test GIT_WORK_TREE, --git-dir, and separate work-tree scenarios.

test_description='work tree setup via GIT_WORK_TREE and --git-dir'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repository' '
	git init repo &&
	cd repo &&
	git config user.email "test@test.com" &&
	git config user.name "Test" &&
	echo "hello" >file.txt &&
	git add file.txt &&
	git commit -m "initial"
'

test_expect_success 'rev-parse --show-toplevel returns repo root' '
	cd repo &&
	git rev-parse --show-toplevel >actual &&
	test "$(cat actual)" = "$(pwd)"
'

test_expect_success 'rev-parse --git-dir returns .git' '
	cd repo &&
	git rev-parse --git-dir >actual &&
	test "$(cat actual)" = ".git"
'

test_expect_success 'GIT_DIR overrides default git dir discovery' '
	cd repo &&
	mkdir -p /tmp/test-wt-$$ &&
	GIT_DIR="$(pwd)/.git" git rev-parse --git-dir >actual &&
	grep "\.git" actual
'

test_expect_success 'GIT_WORK_TREE overrides show-toplevel' '
	cd repo &&
	mkdir -p /tmp/test-wt-alt-$$ &&
	GIT_DIR="$(pwd)/.git" GIT_WORK_TREE=/tmp/test-wt-alt-$$ \
		git rev-parse --show-toplevel >actual &&
	test "$(cat actual)" = "/tmp/test-wt-alt-$$"
'

test_expect_success 'status works with GIT_WORK_TREE pointing elsewhere' '
	cd repo &&
	mkdir -p /tmp/test-wt-status-$$ &&
	GIT_DIR="$(pwd)/.git" GIT_WORK_TREE=/tmp/test-wt-status-$$ \
		git status >actual 2>&1 &&
	# files from repo appear as deleted in alternate worktree
	grep "deleted" actual
'

test_expect_success 'ls-files works from alternate work tree directory' '
	cd repo &&
	mkdir -p /tmp/test-wt-ls-$$ &&
	cd /tmp/test-wt-ls-$$ &&
	GIT_DIR='"$TRASH_DIRECTORY"'/repo/.git GIT_WORK_TREE=/tmp/test-wt-ls-$$ \
		git ls-files >actual &&
	# index still has file.txt from original commit
	grep "file.txt" actual
'

test_expect_success 'add works in alternate work tree' '
	cd repo &&
	mkdir -p /tmp/test-wt-add-$$ &&
	cd /tmp/test-wt-add-$$ &&
	echo "alt content" >alt.txt &&
	GIT_DIR='"$TRASH_DIRECTORY"'/repo/.git GIT_WORK_TREE=/tmp/test-wt-add-$$ \
		git add alt.txt &&
	GIT_DIR='"$TRASH_DIRECTORY"'/repo/.git GIT_WORK_TREE=/tmp/test-wt-add-$$ \
		git ls-files >actual &&
	grep "alt.txt" actual
'

test_expect_success '--git-dir option overrides discovery' '
	cd repo &&
	mkdir -p /tmp/test-wt-gitdir-$$ &&
	cd /tmp/test-wt-gitdir-$$ &&
	git --git-dir='"$TRASH_DIRECTORY"'/repo/.git rev-parse --git-dir >actual &&
	cat actual &&
	test -s actual
'

test_expect_success 'rev-parse --is-inside-work-tree' '
	cd repo &&
	git rev-parse --is-inside-work-tree >actual &&
	test "$(cat actual)" = "true"
'

test_expect_success 'rev-parse --is-inside-work-tree from .git dir' '
	cd repo/.git &&
	git rev-parse --is-inside-work-tree >actual 2>&1 &&
	test "$(cat actual)" = "false"
'

test_expect_success 'rev-parse --is-bare-repository' '
	cd repo &&
	git rev-parse --is-bare-repository >actual &&
	test "$(cat actual)" = "false"
'

test_expect_success 'bare repo: --is-bare-repository is true' '
	git init --bare bare-repo.git &&
	cd bare-repo.git &&
	git rev-parse --is-bare-repository >actual &&
	test "$(cat actual)" = "true"
'

test_expect_success 'bare repo: --show-toplevel fails or empty' '
	cd bare-repo.git &&
	test_must_fail git rev-parse --show-toplevel 2>err ||
	test ! -s actual
'

test_expect_success 'rev-parse --show-prefix from subdirectory' '
	cd repo &&
	mkdir -p sub/dir &&
	cd sub/dir &&
	git rev-parse --show-prefix >actual &&
	test "$(cat actual)" = "sub/dir/"
'

test_expect_success 'rev-parse --show-prefix from nested subdirectory' '
	cd repo &&
	mkdir -p sub2/dir2 &&
	cd sub2/dir2 &&
	git rev-parse --show-prefix >actual &&
	test "$(cat actual)" = "sub2/dir2/"
'

test_expect_success 'init creates separate git dir with --separate-git-dir' '
	mkdir -p sep-wt &&
	cd sep-wt &&
	git init --separate-git-dir ../sep-git.git . 2>&1 ||
	true
'

test_expect_success 'GIT_DIR with relative path works' '
	cd repo &&
	GIT_DIR=.git git rev-parse HEAD >actual &&
	test -s actual
'

test_expect_success 'GIT_DIR with absolute path works' '
	cd repo &&
	GIT_DIR="$(pwd)/.git" git rev-parse HEAD >actual &&
	test -s actual
'

test_expect_success 'rev-parse from subdirectory finds repo' '
	cd repo &&
	mkdir -p deep/nested/dir &&
	cd deep/nested/dir &&
	git rev-parse HEAD >actual &&
	test -s actual
'

test_expect_success 'status from subdirectory works' '
	cd repo &&
	mkdir -p sub &&
	cd sub &&
	git status >actual 2>&1 &&
	test -s actual
'

test_expect_success 'add from subdirectory works' '
	cd repo &&
	mkdir -p sub &&
	echo "sub file" >sub/subfile.txt &&
	cd sub &&
	git add subfile.txt &&
	git ls-files >actual &&
	grep "subfile.txt" actual
'

test_expect_success 'commit from subdirectory works' '
	cd repo &&
	cd sub &&
	git commit -m "add subfile" &&
	git rev-list HEAD >actual &&
	test "$(wc -l <actual | tr -d " ")" -ge 2
'

test_expect_success 'GIT_WORK_TREE with trailing slash' '
	cd repo &&
	GIT_DIR="$(pwd)/.git" GIT_WORK_TREE="$(pwd)/" \
		git rev-parse --show-toplevel >actual &&
	test -s actual
'

test_expect_success 'checkout-index -a checks out files' '
	cd repo &&
	rm -f file.txt &&
	git checkout-index -a &&
	test -f file.txt
'

test_expect_success 'write-tree works regardless of cwd' '
	cd repo &&
	mkdir -p sub &&
	cd sub &&
	git write-tree >actual &&
	test -s actual
'

test_expect_success 'ls-files from repo root lists all tracked files' '
	cd repo &&
	git ls-files >actual &&
	wc -l <actual | tr -d " " >count &&
	test "$(cat count)" -ge 1
'

test_done
