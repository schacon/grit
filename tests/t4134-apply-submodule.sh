#!/bin/sh
#
# Ported subset from git/t/t4134-apply-submodule.sh

test_description='git apply creation and deletion patterns'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	git init repo && cd repo &&
	echo "initial" >file &&
	git add file &&
	git commit -m initial
'

test_expect_success 'creating a new file via patch and then removing it' '
	cd repo &&
	cat >create.patch <<-\EOF &&
	diff --git a/newfile b/newfile
	new file mode 100644
	index 0000000..d95f3ad
	--- /dev/null
	+++ b/newfile
	@@ -0,0 +1 @@
	+content
	EOF
	git apply create.patch &&
	test -f newfile &&
	echo content >expect &&
	test_cmp expect newfile &&
	cat >remove.patch <<-\EOF &&
	diff --git a/newfile b/newfile
	deleted file mode 100644
	index d95f3ad..0000000
	--- a/newfile
	+++ /dev/null
	@@ -1 +0,0 @@
	-content
	EOF
	git apply remove.patch &&
	test_path_is_missing newfile
'

test_done
