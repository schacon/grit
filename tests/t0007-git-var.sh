#!/bin/sh
# Ported from git/t/t0007-git-var.sh
# Tests for 'grit var'.

test_description='basic sanity checks for git var'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ── Test environment setup ───────────────────────────────────────────────────

# Set known identity values so ident tests are deterministic.
GIT_AUTHOR_NAME='A U Thor'
GIT_AUTHOR_EMAIL='author@example.com'
GIT_COMMITTER_NAME='C O Mitter'
GIT_COMMITTER_EMAIL='committer@example.com'
export GIT_AUTHOR_NAME GIT_AUTHOR_EMAIL GIT_COMMITTER_NAME GIT_COMMITTER_EMAIL

# Helper: safely unset one or more env variables.
sane_unset () {
	for var do
		unset "$var" 2>/dev/null || true
	done
}

# Helper: unset all editor-related variables.
sane_unset_all_editors () {
	sane_unset GIT_EDITOR VISUAL EDITOR
}

# Helper: expect a specific exit code.
test_expect_code () {
	local expected="$1"
	shift
	local actual
	"$@"
	actual=$?
	test "$actual" -eq "$expected"
}

# Helper: set a local git config key and register it for cleanup on EXIT.
# Works because test_expect_success runs each test body in a subshell.
test_config () {
	local key="$1" val="$2"
	git config "$key" "$val" &&
	# shellcheck disable=SC2064
	trap "git config --unset '$key' 2>/dev/null; trap - EXIT" EXIT
}

# Initialise a git repo in the trash directory.
# Do NOT set user.name/user.email here — identity tests rely on the env vars above.
test_expect_success 'setup repo' '
	git init .
'

# ── Identity variables ───────────────────────────────────────────────────────

test_expect_success 'get GIT_AUTHOR_IDENT' '
	test_tick &&
	echo "$GIT_AUTHOR_NAME <$GIT_AUTHOR_EMAIL> $GIT_AUTHOR_DATE" >expect &&
	git var GIT_AUTHOR_IDENT >actual &&
	test_cmp expect actual
'

test_expect_success 'get GIT_COMMITTER_IDENT' '
	test_tick &&
	echo "$GIT_COMMITTER_NAME <$GIT_COMMITTER_EMAIL> $GIT_COMMITTER_DATE" >expect &&
	git var GIT_COMMITTER_IDENT >actual &&
	test_cmp expect actual
'

# Strict mode: fail when neither env vars nor config provide the identity.
test_expect_success 'requested identities are strict' '
	(
		sane_unset GIT_COMMITTER_NAME GIT_COMMITTER_EMAIL &&
		test_must_fail git var GIT_COMMITTER_IDENT
	)
'

# ── Default branch ───────────────────────────────────────────────────────────

test_expect_success 'get GIT_DEFAULT_BRANCH without configuration' '
	(
		sane_unset GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME &&
		git init defbranch &&
		git -C defbranch symbolic-ref --short HEAD >expect &&
		git var GIT_DEFAULT_BRANCH >actual &&
		test_cmp expect actual
	)
'

test_expect_success 'get GIT_DEFAULT_BRANCH with configuration' '
	test_config init.defaultbranch foo &&
	(
		sane_unset GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME &&
		echo foo >expect &&
		git var GIT_DEFAULT_BRANCH >actual &&
		test_cmp expect actual
	)
'

# ── Editor variables ─────────────────────────────────────────────────────────

test_expect_success 'get GIT_EDITOR without configuration' '
	(
		sane_unset_all_editors &&
		test_expect_code 1 git var GIT_EDITOR >out &&
		test_must_be_empty out
	)
'

test_expect_success 'get GIT_EDITOR with configuration' '
	test_config core.editor foo &&
	(
		sane_unset_all_editors &&
		echo foo >expect &&
		git var GIT_EDITOR >actual &&
		test_cmp expect actual
	)
'

test_expect_success 'get GIT_EDITOR with environment variable GIT_EDITOR' '
	(
		sane_unset_all_editors &&
		echo bar >expect &&
		GIT_EDITOR=bar git var GIT_EDITOR >actual &&
		test_cmp expect actual
	)
'

test_expect_success 'get GIT_EDITOR with environment variable EDITOR' '
	(
		sane_unset_all_editors &&
		echo bar >expect &&
		EDITOR=bar git var GIT_EDITOR >actual &&
		test_cmp expect actual
	)
'

test_expect_success 'get GIT_EDITOR with configuration and environment variable GIT_EDITOR' '
	test_config core.editor foo &&
	(
		sane_unset_all_editors &&
		echo bar >expect &&
		GIT_EDITOR=bar git var GIT_EDITOR >actual &&
		test_cmp expect actual
	)
'

test_expect_success 'get GIT_EDITOR with configuration and environment variable EDITOR' '
	test_config core.editor foo &&
	(
		sane_unset_all_editors &&
		echo foo >expect &&
		EDITOR=bar git var GIT_EDITOR >actual &&
		test_cmp expect actual
	)
'

# ── Sequence editor ──────────────────────────────────────────────────────────

test_expect_success 'get GIT_SEQUENCE_EDITOR without configuration' '
	(
		sane_unset GIT_SEQUENCE_EDITOR &&
		git var GIT_EDITOR >expect &&
		git var GIT_SEQUENCE_EDITOR >actual &&
		test_cmp expect actual
	)
'

test_expect_success 'get GIT_SEQUENCE_EDITOR with configuration' '
	test_config sequence.editor foo &&
	(
		sane_unset GIT_SEQUENCE_EDITOR &&
		echo foo >expect &&
		git var GIT_SEQUENCE_EDITOR >actual &&
		test_cmp expect actual
	)
'

test_expect_success 'get GIT_SEQUENCE_EDITOR with environment variable' '
	(
		sane_unset GIT_SEQUENCE_EDITOR &&
		echo bar >expect &&
		GIT_SEQUENCE_EDITOR=bar git var GIT_SEQUENCE_EDITOR >actual &&
		test_cmp expect actual
	)
'

test_expect_success 'get GIT_SEQUENCE_EDITOR with configuration and environment variable' '
	test_config sequence.editor foo &&
	(
		sane_unset GIT_SEQUENCE_EDITOR &&
		echo bar >expect &&
		GIT_SEQUENCE_EDITOR=bar git var GIT_SEQUENCE_EDITOR >actual &&
		test_cmp expect actual
	)
'

# ── Shell path ───────────────────────────────────────────────────────────────

test_expect_success 'GIT_SHELL_PATH points to a valid executable' '
	shellpath=$(git var GIT_SHELL_PATH) &&
	test -x "$shellpath"
'

# ── Attribute path variables ─────────────────────────────────────────────────

test_expect_success 'GIT_ATTR_SYSTEM produces expected output' '
	test_must_fail env GIT_ATTR_NOSYSTEM=1 git var GIT_ATTR_SYSTEM &&
	(
		sane_unset GIT_ATTR_NOSYSTEM &&
		systempath=$(git var GIT_ATTR_SYSTEM) &&
		test "$systempath" != ""
	)
'

test_expect_success 'GIT_ATTR_GLOBAL points to the correct location' '
	TRASHDIR="$(pwd)" &&
	globalpath=$(XDG_CONFIG_HOME="$TRASHDIR/.config" git var GIT_ATTR_GLOBAL) &&
	test "$globalpath" = "$TRASHDIR/.config/git/attributes" &&
	(
		sane_unset XDG_CONFIG_HOME &&
		globalpath=$(HOME="$TRASHDIR" git var GIT_ATTR_GLOBAL) &&
		test "$globalpath" = "$TRASHDIR/.config/git/attributes"
	)
'

# ── Config path variables ────────────────────────────────────────────────────

test_expect_success 'GIT_CONFIG_SYSTEM points to the correct location' '
	test_must_fail env GIT_CONFIG_NOSYSTEM=1 git var GIT_CONFIG_SYSTEM &&
	(
		sane_unset GIT_CONFIG_NOSYSTEM &&
		systempath=$(git var GIT_CONFIG_SYSTEM) &&
		test "$systempath" != "" &&
		systempath=$(GIT_CONFIG_SYSTEM=/dev/null git var GIT_CONFIG_SYSTEM) &&
		test "$systempath" = "/dev/null"
	)
'

test_expect_success 'GIT_CONFIG_GLOBAL points to the correct location' '
	TRASHDIR="$(pwd)" &&
	HOME="$TRASHDIR" XDG_CONFIG_HOME="$TRASHDIR/foo" git var GIT_CONFIG_GLOBAL >actual &&
	printf "%s\n" "$TRASHDIR/foo/git/config" "$TRASHDIR/.gitconfig" >expected &&
	test_cmp expected actual &&
	(
		sane_unset XDG_CONFIG_HOME &&
		HOME="$TRASHDIR" git var GIT_CONFIG_GLOBAL >actual &&
		printf "%s\n" "$TRASHDIR/.config/git/config" "$TRASHDIR/.gitconfig" >expected &&
		test_cmp expected actual &&
		globalpath=$(GIT_CONFIG_GLOBAL=/dev/null git var GIT_CONFIG_GLOBAL) &&
		test "$globalpath" = "/dev/null"
	)
'

# ── Listing (`-l`) ───────────────────────────────────────────────────────────

# Check a representative variable rather than the full output.
test_expect_success 'git var -l lists variables' '
	test_tick &&
	git var -l >actual &&
	echo "$GIT_AUTHOR_NAME <$GIT_AUTHOR_EMAIL> $GIT_AUTHOR_DATE" >expect &&
	sed -n "s/^GIT_AUTHOR_IDENT=//p" <actual >actual.author &&
	test_cmp expect actual.author
'

test_expect_success 'git var -l lists config' '
	git var -l >actual &&
	echo false >expect &&
	sed -n "s/^core\.bare=//p" <actual >actual.bare &&
	test_cmp expect actual.bare
'

test_expect_success 'listing and asking for variables are exclusive' '
	test_must_fail git var -l GIT_COMMITTER_IDENT
'

test_expect_success 'git var -l works even without HOME' '
	(
		XDG_CONFIG_HOME= &&
		export XDG_CONFIG_HOME &&
		unset HOME &&
		git var -l
	)
'

test_done
