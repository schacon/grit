#!/bin/sh
#
# Copyright (c) 2009 Giuseppe Bilotta
#

test_description='git-apply basic patch application'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

cat > patch1.patch <<\EOF
diff --git a/main.c b/main.c
new file mode 100644
--- /dev/null
+++ b/main.c
@@ -0,0 +1,22 @@
+#include <stdio.h>
+
+void print_int(int num);
+int func(int num);
+
+int main() {
+       int i;
+
+       for (i = 0; i < 10; i++) {
+               print_int(func(i)); /* stuff */
+       }
+
+       return 0;
+}
+
+int func(int num) {
+       return num * num;
+}
+
+void print_int(int num) {
+       printf("%d", num);
+}
EOF

test_expect_success 'setup' '
	git init repo && cd repo
'

test_expect_success 'file creation' '
	cd repo &&
	git apply ../patch1.patch &&
	test -f main.c
'

test_done
