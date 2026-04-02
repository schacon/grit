#!/bin/sh
# Tests for initial repository setup, .git directory structure, and default config.

test_description='repository setup and .git structure'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ── Basic init ───────────────────────────────────────────────────────────────

test_expect_success 'init creates .git directory' '
	git init setup-basic &&
	test_path_is_dir setup-basic/.git
'

test_expect_success 'init creates objects directory' '
	test_path_is_dir setup-basic/.git/objects
'

test_expect_success 'init creates objects/info directory' '
	test_path_is_dir setup-basic/.git/objects/info
'

test_expect_success 'init creates objects/pack directory' '
	test_path_is_dir setup-basic/.git/objects/pack
'

test_expect_success 'init creates refs directory' '
	test_path_is_dir setup-basic/.git/refs
'

test_expect_success 'init creates refs/heads directory' '
	test_path_is_dir setup-basic/.git/refs/heads
'

test_expect_success 'init creates refs/tags directory' '
	test_path_is_dir setup-basic/.git/refs/tags
'

test_expect_success 'init creates HEAD file' '
	test_path_is_file setup-basic/.git/HEAD
'

test_expect_success 'HEAD points to refs/heads/master by default' '
	head_content=$(cat setup-basic/.git/HEAD) &&
	test "$head_content" = "ref: refs/heads/master"
'

test_expect_success 'init creates config file' '
	test_path_is_file setup-basic/.git/config
'

test_expect_success 'config file contains repositoryformatversion' '
	grep "repositoryformatversion" setup-basic/.git/config
'

# ── Init with custom branch name ────────────────────────────────────────────

test_expect_success 'init -b sets custom initial branch' '
	git init -b main setup-custom-branch &&
	head_content=$(cat setup-custom-branch/.git/HEAD) &&
	test "$head_content" = "ref: refs/heads/main"
'

test_expect_success 'init -b with unusual name works' '
	git init -b develop setup-develop &&
	head_content=$(cat setup-develop/.git/HEAD) &&
	test "$head_content" = "ref: refs/heads/develop"
'

# ── Init in existing directory ───────────────────────────────────────────────

test_expect_success 'init in existing empty directory succeeds' '
	mkdir existing-dir &&
	git init existing-dir &&
	test_path_is_dir existing-dir/.git
'

test_expect_success 'reinit in already-initialized directory succeeds' '
	git init existing-dir &&
	test_path_is_dir existing-dir/.git
'

test_expect_success 'reinit does not error on already-init directory' '
	git init existing-dir 2>err &&
	test_path_is_dir existing-dir/.git
'

# ── Bare init ────────────────────────────────────────────────────────────────

test_expect_success 'init --bare creates bare repository' '
	git init --bare setup-bare.git &&
	test_path_is_dir setup-bare.git/objects &&
	test_path_is_dir setup-bare.git/refs &&
	test_path_is_file setup-bare.git/HEAD
'

test_expect_success 'bare repository has no .git subdirectory' '
	test_path_is_missing setup-bare.git/.git
'

test_expect_success 'bare repository config has bare = true' '
	grep "bare = true" setup-bare.git/config
'

# ── Default config values ───────────────────────────────────────────────────

test_expect_success 'repositoryformatversion is 0' '
	cd setup-basic &&
	val=$(git config --get core.repositoryformatversion) &&
	test "$val" = "0"
'

test_expect_success 'filemode is set in config' '
	cd setup-basic &&
	git config --get core.filemode >/dev/null
'

test_expect_success 'bare is false in non-bare repo' '
	cd setup-basic &&
	val=$(git config --get core.bare) &&
	test "$val" = "false"
'

test_expect_success 'logallrefupdates is set in non-bare repo' '
	cd setup-basic &&
	git config --get core.logallrefupdates >/dev/null
'

# ── Working tree after init ─────────────────────────────────────────────────

test_expect_success 'empty repo status shows no commits yet' '
	git init empty-status-repo &&
	cd empty-status-repo &&
	git status >../status-out 2>&1 &&
	grep -i "no commits yet" ../status-out
'

test_expect_success 'empty repo log produces no output' '
	git init empty-log-repo &&
	cd empty-log-repo &&
	git log >actual 2>&1 &&
	test_must_be_empty actual
'

test_expect_success 'empty repo ls-files returns nothing' '
	cd setup-basic &&
	git ls-files >actual &&
	test_must_be_empty actual
'

# ── First commit creates branch ref ─────────────────────────────────────────

test_expect_success 'first commit creates refs/heads/master' '
	cd setup-basic &&
	git config user.name "Setup Test" &&
	git config user.email "setup@test.com" &&
	echo "first" >first.txt &&
	git add first.txt &&
	git commit -m "first commit" &&
	test_path_is_file .git/refs/heads/master
'

test_expect_success 'HEAD resolves to a commit after first commit' '
	cd setup-basic &&
	oid=$(git rev-parse HEAD) &&
	type=$(git cat-file -t "$oid") &&
	test "$type" = "commit"
'

test_expect_success 'rev-parse HEAD matches refs/heads/master' '
	cd setup-basic &&
	head_oid=$(git rev-parse HEAD) &&
	master_oid=$(git rev-parse refs/heads/master) &&
	test "$head_oid" = "$master_oid"
'

# ── Separate git dir ────────────────────────────────────────────────────────

test_expect_success 'objects/info and objects/pack exist after init' '
	git init verify-obj-dirs &&
	test_path_is_dir verify-obj-dirs/.git/objects/info &&
	test_path_is_dir verify-obj-dirs/.git/objects/pack
'

# ── Quiet init ──────────────────────────────────────────────────────────────

test_expect_success 'init -q suppresses output' '
	git init -q setup-quiet >actual 2>&1 &&
	test_must_be_empty actual
'

test_done
