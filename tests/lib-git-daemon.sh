# Shell library to run git-daemon in tests.
#
# Usage:
#   . ./test-lib.sh
#   . "$TEST_DIRECTORY"/lib-git-daemon.sh
#   start_git_daemon
#
#   test_expect_success '...' '
#       ...
#   '
#
#   test_done

if ! test_have_prereq PIPE
then
	skip_all="file system does not support FIFOs"
	test_done
fi

test_set_port LIB_GIT_DAEMON_PORT

GIT_DAEMON_PID=
GIT_DAEMON_PIDFILE="$PWD"/daemon.pid
GIT_DAEMON_DOCUMENT_ROOT_PATH="$PWD"/repo
GIT_DAEMON_HOST_PORT=127.0.0.1:$LIB_GIT_DAEMON_PORT
GIT_DAEMON_URL=git://$GIT_DAEMON_HOST_PORT

registered_stop_git_daemon_atexit_handler=
start_git_daemon() {
	# Stop any previously running daemon
	stop_git_daemon 2>/dev/null

	mkdir -p "$GIT_DAEMON_DOCUMENT_ROOT_PATH"

	# Cleanup is handled by explicit stop_git_daemon calls
	# and test_done cleanup.

	${LIB_GIT_DAEMON_COMMAND:-git daemon} \
		--listen=127.0.0.1 --port="$LIB_GIT_DAEMON_PORT" \
		--reuseaddr --verbose --pid-file="$GIT_DAEMON_PIDFILE" \
		--base-path="$GIT_DAEMON_DOCUMENT_ROOT_PATH" \
		"$@" "$GIT_DAEMON_DOCUMENT_ROOT_PATH" \
		>"$PWD/git_daemon_output" 2>&1 &
	GIT_DAEMON_PID=$!

	# Wait for daemon to be ready (up to 5 seconds)
	local tries=0
	while test $tries -lt 50
	do
		if grep -q "Ready to rumble" "$PWD/git_daemon_output" 2>/dev/null
		then
			break
		fi
		sleep 0.1
		tries=$((tries + 1))
	done

	if ! grep -q "Ready to rumble" "$PWD/git_daemon_output" 2>/dev/null
	then
		kill "$GIT_DAEMON_PID" 2>/dev/null
		wait "$GIT_DAEMON_PID" 2>/dev/null
		unset GIT_DAEMON_PID
		skip_all="git daemon failed to start"
		test_done
	fi
}

stop_git_daemon() {
	if test -z "$GIT_DAEMON_PID"
	then
		return
	fi

	# Kill the wrapper process
	kill "$GIT_DAEMON_PID" 2>/dev/null

	# Kill via PID file (the actual forked daemon process)
	if test -f "$GIT_DAEMON_PIDFILE"
	then
		local _dpid
		_dpid=$(cat "$GIT_DAEMON_PIDFILE" 2>/dev/null)
		if test -n "$_dpid"
		then
			kill "$_dpid" 2>/dev/null
			# Wait briefly for it to die
			local _i=0
			while kill -0 "$_dpid" 2>/dev/null && test $_i -lt 10; do
				sleep 0.1
				_i=$((_i + 1))
			done
			# Force kill if still alive
			kill -9 "$_dpid" 2>/dev/null
		fi
		rm -f "$GIT_DAEMON_PIDFILE"
	fi

	wait "$GIT_DAEMON_PID" 2>/dev/null
	GIT_DAEMON_PID=
	rm -f "$PWD/git_daemon_output"
}

# A stripped-down version of a netcat client
fake_nc() {
	if ! test_have_prereq FAKENC
	then
		echo "fake_nc: need to declare FAKENC prerequisite" >&2
		return 127
	fi
	perl -Mstrict -MIO::Socket::INET -e '
		my $s = IO::Socket::INET->new(shift)
			or die "unable to open socket: $!";
		print $s <STDIN>;
		$s->shutdown(1);
		print <$s>;
	' "$@"
}

test_lazy_prereq FAKENC '
	perl -MIO::Socket::INET -e "exit 0"
'
