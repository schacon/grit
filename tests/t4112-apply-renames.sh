#!/bin/sh
#
# Ported subset from git/t/t4112-apply-renames.sh
# Tests git apply --check and basic patch application

test_description='git apply basic file creation and modification'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo && cd repo &&
	echo "This is a simple readme file." >README &&
	git add README &&
	git commit -m "initial"
'

test_expect_success 'check valid patch' '
	cd repo &&
	cat >patch <<-\EOF &&
	diff --git a/README b/README
	--- a/README
	+++ b/README
	@@ -1 +1,4 @@
	 This is a simple readme file.
	+And we add a few
	+lines at the
	+end of it.
	EOF
	git apply --check patch
'

test_expect_success 'apply content modification patch' '
	cd repo &&
	git apply patch &&
	test_write_lines "This is a simple readme file." \
		"And we add a few" "lines at the" "end of it." >expect &&
	test_cmp expect README
'

test_expect_success 'apply --stat on modification patch' '
	cd repo &&
	git apply --stat patch >output &&
	test_grep "README" output
'

test_done
