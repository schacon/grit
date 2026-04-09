#!/usr/bin/env python3
"""One-off: build tests/t5558-clone-bundle-uri.sh from upstream + inserts."""
from pathlib import Path

src = Path("/Users/schacon/grit/git/t/t5558-clone-bundle-uri.sh").read_text()
lines = src.splitlines(keepends=True)

header_comment = """#
# Upstream: t5558-clone-bundle-uri.sh
# Tests fetching bundles with --bundle-uri (file and HTTP).
# Requires git >= 2.45 for refs/bundles/ support.
#

"""

bundle_block = r"""# These tests need real git (grit doesn't support --bundle-uri yet)
REAL_GIT="$(command -v git 2>/dev/null || echo /usr/bin/git)"
for _p in $(echo "$PATH" | tr ':' ' '); do
	if test -x "$_p/git" && ! grep -q 'grit' "$_p/git" 2>/dev/null; then
		REAL_GIT="$_p/git"
		break
	fi
done
cat >"$TRASH_DIRECTORY/.bin/git" <<EOFWRAP
#!/bin/sh
exec "$REAL_GIT" "\$@"
EOFWRAP
chmod +x "$TRASH_DIRECTORY/.bin/git"

# Check if bundle-uri creates refs/bundles/ — requires git >= 2.45
test_expect_success 'check bundle-uri refs/bundles support' '
	git init check-bundle &&
	(cd check-bundle &&
	 echo content >file &&
	 git add file &&
	 git commit -m initial &&
	 git bundle create ../check.bundle HEAD
	) &&
	git clone --bundle-uri="$(pwd)/check.bundle" check-bundle check-clone 2>err &&
	if ! git -C check-clone for-each-ref --format="%(refname)" | grep -q "refs/bundles/"
	then
		skip_all="git $(git --version) does not store refs/bundles/ from bundle-uri (need >= 2.45)"
		test_done
	fi
'

"""

out = [lines[0], lines[1], header_comment] + lines[2:6] + [bundle_block] + lines[6:]
dst = Path("/Users/schacon/grit/tests/t5558-clone-bundle-uri.sh")
dst.write_text("".join(out))
print("Wrote", dst)
