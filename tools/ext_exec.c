// ext_exec: run a command under the SCHED_EXT policy.
//
//   ext_exec make -j20
//
// Children inherit the policy, so a whole batch job (make and every
// compiler it spawns) is scheduled by the sched_ext scheduler in partial
// mode, while everything else stays on EEVDF. With no sched_ext scheduler
// loaded, the kernel runs SCHED_EXT tasks as normal fair tasks.
#define _GNU_SOURCE
#include <sched.h>
#include <stdio.h>
#include <unistd.h>

#ifndef SCHED_EXT
#define SCHED_EXT 7
#endif

int main(int argc, char **argv)
{
	if (argc < 2) {
		fprintf(stderr, "usage: %s COMMAND [ARGS...]\n", argv[0]);
		return 2;
	}
	struct sched_param param = { .sched_priority = 0 };
	if (sched_setscheduler(0, SCHED_EXT, &param)) {
		perror("sched_setscheduler(SCHED_EXT)");
		return 1;
	}
	execvp(argv[1], argv + 1);
	perror(argv[1]);
	return 127;
}
