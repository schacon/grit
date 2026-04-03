#!/bin/sh

test_description='test GIT_CEILING_DIRECTORIES'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# We need a git repo in the trash directory for the prefix tests to work.
test_expect_success 'setup' '
	git init
'

TRASH_ROOT="$TRASH_DIRECTORY"
ROOT_PARENT=$(dirname "$TRASH_ROOT")

test_expect_success 'no_ceil: prefix is empty at root' '
	unset GIT_CEILING_DIRECTORIES &&
	echo "" >expect &&
	git rev-parse --show-prefix >actual &&
	test_cmp expect actual
'

test_expect_success 'ceil_empty: prefix is empty at root' '
	GIT_CEILING_DIRECTORIES="" &&
	export GIT_CEILING_DIRECTORIES &&
	echo "" >expect &&
	git rev-parse --show-prefix >actual &&
	test_cmp expect actual
'

test_expect_success 'ceil_at_parent: prefix is empty at root' '
	GIT_CEILING_DIRECTORIES="'"$ROOT_PARENT"'" &&
	export GIT_CEILING_DIRECTORIES &&
	echo "" >expect &&
	git rev-parse --show-prefix >actual &&
	test_cmp expect actual
'

test_expect_success 'ceil_at_parent_slash: prefix is empty at root' '
	GIT_CEILING_DIRECTORIES="'"$ROOT_PARENT/"'" &&
	export GIT_CEILING_DIRECTORIES &&
	echo "" >expect &&
	git rev-parse --show-prefix >actual &&
	test_cmp expect actual
'

test_expect_success 'ceil_at_trash: prefix is empty at root' '
	GIT_CEILING_DIRECTORIES="'"$TRASH_ROOT"'" &&
	export GIT_CEILING_DIRECTORIES &&
	echo "" >expect &&
	git rev-parse --show-prefix >actual &&
	test_cmp expect actual
'

test_expect_success 'ceil_at_trash_slash: prefix is empty at root' '
	GIT_CEILING_DIRECTORIES="'"$TRASH_ROOT/"'" &&
	export GIT_CEILING_DIRECTORIES &&
	echo "" >expect &&
	git rev-parse --show-prefix >actual &&
	test_cmp expect actual
'

test_expect_success 'ceil_at_sub: prefix is empty at root' '
	GIT_CEILING_DIRECTORIES="'"$TRASH_ROOT/sub"'" &&
	export GIT_CEILING_DIRECTORIES &&
	echo "" >expect &&
	git rev-parse --show-prefix >actual &&
	test_cmp expect actual
'

test_expect_success 'ceil_at_sub_slash: prefix is empty at root' '
	GIT_CEILING_DIRECTORIES="'"$TRASH_ROOT/sub/"'" &&
	export GIT_CEILING_DIRECTORIES &&
	echo "" >expect &&
	git rev-parse --show-prefix >actual &&
	test_cmp expect actual
'

test_expect_success 'subdir_no_ceil: prefix is sub/dir/' '
	mkdir -p sub/dir &&
	unset GIT_CEILING_DIRECTORIES &&
	echo "sub/dir/" >expect &&
	(cd sub/dir && git rev-parse --show-prefix) >actual &&
	test_cmp expect actual
'

test_expect_success 'subdir_ceil_empty: prefix is sub/dir/' '
	mkdir -p sub/dir &&
	GIT_CEILING_DIRECTORIES="" &&
	export GIT_CEILING_DIRECTORIES &&
	echo "sub/dir/" >expect &&
	(cd sub/dir && git rev-parse --show-prefix) >actual &&
	test_cmp expect actual
'

test_expect_success 'subdir_ceil_at_trash: fails' '
	mkdir -p sub/dir &&
	GIT_CEILING_DIRECTORIES="'"$TRASH_ROOT"'" &&
	export GIT_CEILING_DIRECTORIES &&
	(cd sub/dir && test_must_fail git rev-parse --show-prefix)
'

test_expect_success 'subdir_ceil_at_trash_slash: fails' '
	mkdir -p sub/dir &&
	GIT_CEILING_DIRECTORIES="'"$TRASH_ROOT/"'" &&
	export GIT_CEILING_DIRECTORIES &&
	(cd sub/dir && test_must_fail git rev-parse --show-prefix)
'

test_expect_success 'subdir_ceil_at_sub: fails' '
	mkdir -p sub/dir &&
	GIT_CEILING_DIRECTORIES="'"$TRASH_ROOT/sub"'" &&
	export GIT_CEILING_DIRECTORIES &&
	(cd sub/dir && test_must_fail git rev-parse --show-prefix)
'

test_expect_success 'subdir_ceil_at_sub_slash: fails' '
	mkdir -p sub/dir &&
	GIT_CEILING_DIRECTORIES="'"$TRASH_ROOT/sub/"'" &&
	export GIT_CEILING_DIRECTORIES &&
	(cd sub/dir && test_must_fail git rev-parse --show-prefix)
'

test_expect_success 'subdir_ceil_at_subdir: prefix is sub/dir/' '
	mkdir -p sub/dir &&
	GIT_CEILING_DIRECTORIES="'"$TRASH_ROOT/sub/dir"'" &&
	export GIT_CEILING_DIRECTORIES &&
	echo "sub/dir/" >expect &&
	(cd sub/dir && git rev-parse --show-prefix) >actual &&
	test_cmp expect actual
'

test_expect_success 'subdir_ceil_at_subdir_slash: prefix is sub/dir/' '
	mkdir -p sub/dir &&
	GIT_CEILING_DIRECTORIES="'"$TRASH_ROOT/sub/dir/"'" &&
	export GIT_CEILING_DIRECTORIES &&
	echo "sub/dir/" >expect &&
	(cd sub/dir && git rev-parse --show-prefix) >actual &&
	test_cmp expect actual
'

test_expect_success 'subdir_ceil_at_su: prefix is sub/dir/' '
	mkdir -p sub/dir &&
	GIT_CEILING_DIRECTORIES="'"$TRASH_ROOT/su"'" &&
	export GIT_CEILING_DIRECTORIES &&
	echo "sub/dir/" >expect &&
	(cd sub/dir && git rev-parse --show-prefix) >actual &&
	test_cmp expect actual
'

test_expect_success 'subdir_ceil_at_su_slash: prefix is sub/dir/' '
	mkdir -p sub/dir &&
	GIT_CEILING_DIRECTORIES="'"$TRASH_ROOT/su/"'" &&
	export GIT_CEILING_DIRECTORIES &&
	echo "sub/dir/" >expect &&
	(cd sub/dir && git rev-parse --show-prefix) >actual &&
	test_cmp expect actual
'

test_expect_success 'subdir_ceil_at_sub_di: prefix is sub/dir/' '
	mkdir -p sub/dir &&
	GIT_CEILING_DIRECTORIES="'"$TRASH_ROOT/sub/di"'" &&
	export GIT_CEILING_DIRECTORIES &&
	echo "sub/dir/" >expect &&
	(cd sub/dir && git rev-parse --show-prefix) >actual &&
	test_cmp expect actual
'

test_expect_success 'subdir_ceil_at_subdi: prefix is sub/dir/' '
	mkdir -p sub/dir &&
	GIT_CEILING_DIRECTORIES="'"$TRASH_ROOT/subdi"'" &&
	export GIT_CEILING_DIRECTORIES &&
	echo "sub/dir/" >expect &&
	(cd sub/dir && git rev-parse --show-prefix) >actual &&
	test_cmp expect actual
'

test_expect_success 'second_of_two: fails' '
	mkdir -p sub/dir &&
	GIT_CEILING_DIRECTORIES="/foo:'"$TRASH_ROOT/sub"'" &&
	export GIT_CEILING_DIRECTORIES &&
	(cd sub/dir && test_must_fail git rev-parse --show-prefix)
'

test_expect_success 'first_of_two: fails' '
	mkdir -p sub/dir &&
	GIT_CEILING_DIRECTORIES="'"$TRASH_ROOT/sub"':/bar" &&
	export GIT_CEILING_DIRECTORIES &&
	(cd sub/dir && test_must_fail git rev-parse --show-prefix)
'

test_expect_success 'second_of_three: fails' '
	mkdir -p sub/dir &&
	GIT_CEILING_DIRECTORIES="/foo:'"$TRASH_ROOT/sub"':/bar" &&
	export GIT_CEILING_DIRECTORIES &&
	(cd sub/dir && test_must_fail git rev-parse --show-prefix)
'

test_expect_success 'git_dir_specified: prefix is empty' '
	mkdir -p sub/dir &&
	GIT_CEILING_DIRECTORIES="'"$TRASH_ROOT/sub"'" &&
	GIT_DIR="'"$TRASH_ROOT/.git"'" &&
	export GIT_CEILING_DIRECTORIES GIT_DIR &&
	echo "" >expect &&
	(cd sub/dir && git rev-parse --show-prefix) >actual &&
	unset GIT_DIR &&
	test_cmp expect actual
'

test_expect_success 'sd_no_ceil: prefix is s/d/' '
	mkdir -p s/d &&
	unset GIT_CEILING_DIRECTORIES &&
	echo "s/d/" >expect &&
	(cd s/d && git rev-parse --show-prefix) >actual &&
	test_cmp expect actual
'

test_expect_success 'sd_ceil_empty: prefix is s/d/' '
	mkdir -p s/d &&
	GIT_CEILING_DIRECTORIES="" &&
	export GIT_CEILING_DIRECTORIES &&
	echo "s/d/" >expect &&
	(cd s/d && git rev-parse --show-prefix) >actual &&
	test_cmp expect actual
'

test_expect_success 'sd_ceil_at_trash: fails' '
	mkdir -p s/d &&
	GIT_CEILING_DIRECTORIES="'"$TRASH_ROOT"'" &&
	export GIT_CEILING_DIRECTORIES &&
	(cd s/d && test_must_fail git rev-parse --show-prefix)
'

test_expect_success 'sd_ceil_at_trash_slash: fails' '
	mkdir -p s/d &&
	GIT_CEILING_DIRECTORIES="'"$TRASH_ROOT/"'" &&
	export GIT_CEILING_DIRECTORIES &&
	(cd s/d && test_must_fail git rev-parse --show-prefix)
'

test_expect_success 'sd_ceil_at_s: fails' '
	mkdir -p s/d &&
	GIT_CEILING_DIRECTORIES="'"$TRASH_ROOT/s"'" &&
	export GIT_CEILING_DIRECTORIES &&
	(cd s/d && test_must_fail git rev-parse --show-prefix)
'

test_expect_success 'sd_ceil_at_s_slash: fails' '
	mkdir -p s/d &&
	GIT_CEILING_DIRECTORIES="'"$TRASH_ROOT/s/"'" &&
	export GIT_CEILING_DIRECTORIES &&
	(cd s/d && test_must_fail git rev-parse --show-prefix)
'

test_expect_success 'sd_ceil_at_sd: prefix is s/d/' '
	mkdir -p s/d &&
	GIT_CEILING_DIRECTORIES="'"$TRASH_ROOT/s/d"'" &&
	export GIT_CEILING_DIRECTORIES &&
	echo "s/d/" >expect &&
	(cd s/d && git rev-parse --show-prefix) >actual &&
	test_cmp expect actual
'

test_expect_success 'sd_ceil_at_sd_slash: prefix is s/d/' '
	mkdir -p s/d &&
	GIT_CEILING_DIRECTORIES="'"$TRASH_ROOT/s/d/"'" &&
	export GIT_CEILING_DIRECTORIES &&
	echo "s/d/" >expect &&
	(cd s/d && git rev-parse --show-prefix) >actual &&
	test_cmp expect actual
'

test_expect_success 'sd_ceil_at_su: prefix is s/d/' '
	mkdir -p s/d &&
	GIT_CEILING_DIRECTORIES="'"$TRASH_ROOT/su"'" &&
	export GIT_CEILING_DIRECTORIES &&
	echo "s/d/" >expect &&
	(cd s/d && git rev-parse --show-prefix) >actual &&
	test_cmp expect actual
'

test_expect_success 'sd_ceil_at_su_slash: prefix is s/d/' '
	mkdir -p s/d &&
	GIT_CEILING_DIRECTORIES="'"$TRASH_ROOT/su/"'" &&
	export GIT_CEILING_DIRECTORIES &&
	echo "s/d/" >expect &&
	(cd s/d && git rev-parse --show-prefix) >actual &&
	test_cmp expect actual
'

test_expect_success 'sd_ceil_at_s_di: prefix is s/d/' '
	mkdir -p s/d &&
	GIT_CEILING_DIRECTORIES="'"$TRASH_ROOT/s/di"'" &&
	export GIT_CEILING_DIRECTORIES &&
	echo "s/d/" >expect &&
	(cd s/d && git rev-parse --show-prefix) >actual &&
	test_cmp expect actual
'

test_expect_success 'sd_ceil_at_sdi: prefix is s/d/' '
	mkdir -p s/d &&
	GIT_CEILING_DIRECTORIES="'"$TRASH_ROOT/sdi"'" &&
	export GIT_CEILING_DIRECTORIES &&
	echo "s/d/" >expect &&
	(cd s/d && git rev-parse --show-prefix) >actual &&
	test_cmp expect actual
'

test_done
