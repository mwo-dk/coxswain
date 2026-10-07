#!@RCD_SCRIPTS_SHELL@
#
# $NetBSD$
#
# PROVIDE: coxswain_index
# REQUIRE: DAEMON LOGIN
# KEYWORD: shutdown
#
# Coxswain's search helper for one user, from boot: it keeps the file name
# index and the search store current while no Coxswain app is open.
# In /etc/rc.conf:
#
#   coxswain_index=YES
#   coxswain_index_user=alice   the user whose files it indexes, and who runs it

if [ -f /etc/rc.subr ]; then
	. /etc/rc.subr
fi

name="coxswain_index"
rcvar=$name
command="@PREFIX@/bin/coxswain"
pidfile="@VARBASE@/run/${name}/${name}.pid"
start_cmd="coxswain_index_start"

coxswain_index_start()
{
	if [ -z "${coxswain_index_user}" ]; then
		err 1 "Set coxswain_index_user in /etc/rc.conf to the user whose files are indexed."
	fi
	# The helper keeps its index in the user's home, as when the user starts it.
	home=$(getent passwd "${coxswain_index_user}" | cut -d: -f6)
	[ -n "${home}" ] || err 1 "No such user: ${coxswain_index_user}"
	install -d -o "${coxswain_index_user}" -m 755 "@VARBASE@/run/${name}"
	echo "Starting ${name}."
	# su -m keeps sh as the shell; what the helper says goes to syslog.
	su -m "${coxswain_index_user}" -c "unset XDG_CONFIG_HOME XDG_CACHE_HOME XDG_DATA_HOME; \
		HOME='${home}'; export HOME; cd \"\$HOME\" && \
		{ ${command} --index-helper --stay 2>&1 | logger -t ${name} & } ; \
		sleep 1; pgrep -U '${coxswain_index_user}' -f -n '${command} --index-helper --stay' >${pidfile}"
}

load_rc_config $name
run_rc_command "$1"
