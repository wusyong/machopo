// Logs every getenv/secure_getenv a process makes (name, found or not) to
// $ENVPROBE_DIR/<kind>.<pid>, kind being "renderer" or "browser". The file
// is opened at load time, before a sandbox closes the file system. Built
// with -DENVPROBE_DEFAULT_DIR='"..."', that directory is used where the
// variable isn't set: a renderer whose environment is limited doesn't
// have it.
//
// Linux: loaded with LD_PRELOAD, it replaces getenv and secure_getenv.
// macOS: loaded with DYLD_INSERT_LIBRARIES, it interposes getenv (macOS
// has no secure_getenv); calls inside libSystem itself aren't seen.
#define _GNU_SOURCE
#include <dlfcn.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#ifdef __APPLE__
#include <crt_externs.h>
#endif

static int fd = -1;
#ifndef __APPLE__
static char *(*real_getenv)(const char *);
static char *(*real_secure_getenv)(const char *);
#define REAL_GETENV real_getenv
#else
#define REAL_GETENV getenv
#endif

// Whether this process was started as a renderer (`--type=renderer`).
static int is_renderer(void) {
#ifdef __APPLE__
  int argc = *_NSGetArgc();
  char **argv = *_NSGetArgv();
  for (int i = 0; i < argc; i++)
    if (strncmp(argv[i], "--type=renderer", 15) == 0) return 1;
  return 0;
#else
  char cmd[4096] = {0};
  int c = open("/proc/self/cmdline", O_RDONLY);
  ssize_t n = c >= 0 ? read(c, cmd, sizeof cmd - 1) : 0;
  if (c >= 0) close(c);
  for (ssize_t i = 0; i < n; i += strlen(cmd + i) + 1)
    if (strncmp(cmd + i, "--type=renderer", 15) == 0) return 1;
  return 0;
#endif
}

__attribute__((constructor)) static void init(void) {
#ifndef __APPLE__
  real_getenv = dlsym(RTLD_NEXT, "getenv");
  real_secure_getenv = dlsym(RTLD_NEXT, "secure_getenv");
#endif
  const char *dir = REAL_GETENV("ENVPROBE_DIR");
#ifdef ENVPROBE_DEFAULT_DIR
  if (!dir) dir = ENVPROBE_DEFAULT_DIR;
#endif
  if (!dir) return;
  const char *kind = is_renderer() ? "renderer" : "browser";
  char path[4200];
  snprintf(path, sizeof path, "%s/%s.%d", dir, kind, getpid());
  fd = open(path, O_WRONLY | O_CREAT | O_APPEND | O_CLOEXEC, 0644);
}

static void note(const char *how, const char *name, const char *value) {
  if (fd < 0 || !name) return;
  char line[512];
  int len = snprintf(line, sizeof line, "%s %s %s\n", how, name, value ? "set" : "unset");
  if (len > 0) write(fd, line, len > (int)sizeof line ? (int)sizeof line : len);
}

#ifdef __APPLE__
// dyld's interposing: every other image's calls to getenv come here, and
// this one's own call reaches the real one.
static char *logged_getenv(const char *name) {
  char *value = getenv(name);
  note("getenv", name, value);
  return value;
}

__attribute__((used, section("__DATA,__interpose"))) static struct {
  const void *replacement;
  const void *replacee;
} interpose_getenv = {(const void *)logged_getenv, (const void *)getenv};
#else
char *getenv(const char *name) {
  if (!real_getenv) real_getenv = dlsym(RTLD_NEXT, "getenv");
  char *value = real_getenv(name);
  note("getenv", name, value);
  return value;
}

char *secure_getenv(const char *name) {
  if (!real_secure_getenv) real_secure_getenv = dlsym(RTLD_NEXT, "secure_getenv");
  char *value = real_secure_getenv(name);
  note("secure_getenv", name, value);
  return value;
}
#endif
