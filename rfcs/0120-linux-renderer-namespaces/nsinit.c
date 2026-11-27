// A process that, as its first act (one thread), enters new user, network
// and PID namespaces, forks an init (PID 1 of the new PID namespace) that
// forks the "renderer" (PID 2), and passes the renderer's end back out:
// the outermost process waits for the init and dies as the renderer did.
// The renderer sends its credentials over a socket pair to the outermost
// process's parent ("browser"), which reads the pid the kernel puts there.
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <sched.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <sys/socket.h>
#include <sys/wait.h>
#include <unistd.h>

static void write_file(const char *path, const char *text) {
  int fd = open(path, O_WRONLY);
  if (fd < 0 || write(fd, text, strlen(text)) < 0) { perror(path); _exit(90); }
  close(fd);
}

int main(int argc, char **argv) {
  const char *how = argc > 1 ? argv[1] : "exit";
  int sv[2];
  socketpair(AF_UNIX, SOCK_STREAM, 0, sv);
  int one = 1;
  setsockopt(sv[0], SOL_SOCKET, SO_PASSCRED, &one, sizeof one);
  pid_t browser_child = fork();  // what the browser starts
  if (browser_child == 0) {
    close(sv[0]);
    uid_t uid = getuid(); gid_t gid = getgid();
    if (unshare(CLONE_NEWUSER | CLONE_NEWNET | CLONE_NEWPID) != 0) { perror("unshare"); _exit(91); }
    char map[64];
    write_file("/proc/self/setgroups", "deny");
    snprintf(map, sizeof map, "0 %d 1", uid); write_file("/proc/self/uid_map", map);
    snprintf(map, sizeof map, "0 %d 1", gid); write_file("/proc/self/gid_map", map);
    pid_t init = fork();
    if (init == 0) {
      // PID 1 of the new namespace.
      pid_t renderer = fork();
      if (renderer == 0) {
        fprintf(stderr, "renderer: pid %d in its namespace\n", getpid());
        // Send credentials (pid as this namespace sees it).
        struct ucred cred = { .pid = getpid(), .uid = getuid(), .gid = getgid() };
        char cbuf[CMSG_SPACE(sizeof cred)] = {0};
        char byte = 'x';
        struct iovec iov = { &byte, 1 };
        struct msghdr msg = { .msg_iov = &iov, .msg_iovlen = 1, .msg_control = cbuf, .msg_controllen = sizeof cbuf };
        struct cmsghdr *c = CMSG_FIRSTHDR(&msg);
        c->cmsg_level = SOL_SOCKET; c->cmsg_type = SCM_CREDENTIALS; c->cmsg_len = CMSG_LEN(sizeof cred);
        memcpy(CMSG_DATA(c), &cred, sizeof cred);
        if (sendmsg(sv[1], &msg, 0) < 0) perror("sendmsg");
        if (strcmp(how, "abort") == 0) abort();
        _exit(7);
      }
      int status;
      waitpid(renderer, &status, 0);
      // Tell the outer process how the renderer ended: by an exit code
      // that encodes it (a signal can't be raised on PID 1 itself).
      if (WIFSIGNALED(status)) _exit(128 + WTERMSIG(status));
      _exit(WEXITSTATUS(status));
    }
    close(sv[1]);
    int status;
    waitpid(init, &status, 0);
    int code = WIFEXITED(status) ? WEXITSTATUS(status) : 99;
    if (code > 128) { signal(code - 128, SIG_DFL); prctl(PR_SET_DUMPABLE, 0); kill(getpid(), code - 128); }
    _exit(code);
  }
  close(sv[1]);
  // The browser: read the renderer's credentials as this namespace sees them.
  char byte;
  struct ucred cred;
  char cbuf[CMSG_SPACE(sizeof cred)];
  struct iovec iov = { &byte, 1 };
  struct msghdr msg = { .msg_iov = &iov, .msg_iovlen = 1, .msg_control = cbuf, .msg_controllen = sizeof cbuf };
  if (recvmsg(sv[0], &msg, 0) > 0) {
    struct cmsghdr *c = CMSG_FIRSTHDR(&msg);
    memcpy(&cred, CMSG_DATA(c), sizeof cred);
    printf("browser: started pid %d; the renderer's credentials say pid %d uid %d\n", browser_child, cred.pid, cred.uid);
    char path[64]; snprintf(path, sizeof path, "/proc/%d/status", cred.pid);
    FILE *f = fopen(path, "r"); char line[256];
    while (f && fgets(line, sizeof line, f)) if (!strncmp(line, "NSpid", 5) || !strncmp(line, "Name", 4)) printf("  %s", line);
    if (f) fclose(f);
  }
  int status;
  waitpid(browser_child, &status, 0);
  if (WIFSIGNALED(status)) printf("browser: its child ended by signal %d\n", WTERMSIG(status));
  else printf("browser: its child exited %d\n", WEXITSTATUS(status));
  return 0;
}
