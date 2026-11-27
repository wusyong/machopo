"""Which environment variables each process of a run asks for, on Windows.

The Windows counterpart of getenv_log.c: it runs a command under the debug
API (DEBUG_PROCESS, so every process it starts is debugged too, a renderer
in its app container as well), puts a breakpoint on each export that reads
one variable, and records the name asked for. A process is a renderer if
its command line has --type=renderer, and the sandbox's probe if it has
--type=renderer-sandbox-probe (which starts a process itself).
Processes other than the first and machopo's aren't followed.

    python -I rfcs/0119-renderer-environment/getenv_trace.py [--out FILE] -- PROGRAM ARGS...

What a process reads by walking its whole environment isn't seen, only
that it called GetEnvironmentStringsW (the CRT does at start, for the copy
getenv reads, which is why ucrtbase's getenv is broken on too).

For each renderer it also checks that its environment can be read from
outside, as a test would (a handle with only PROCESS_QUERY_LIMITED_INFORMATION
and PROCESS_VM_READ, the environment through the PEB), once it has started
threads: by then it has lowered itself.
"""

import argparse
import collections
import ctypes
import ctypes.wintypes as w
import itertools
import shutil
import struct
import sys

k32 = ctypes.WinDLL("kernel32", use_last_error=True)
ntdll = ctypes.WinDLL("ntdll")

DEBUG_PROCESS = 0x1
INFINITE = 0xFFFFFFFF
DBG_CONTINUE = 0x00010002
DBG_EXCEPTION_NOT_HANDLED = 0x80010001
EXCEPTION_BREAKPOINT = 0x80000003
EXCEPTION_SINGLE_STEP = 0x80000004
PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
PROCESS_VM_READ = 0x0010
# CONTEXT_AMD64 | CONTEXT_CONTROL | CONTEXT_INTEGER
CONTEXT_FLAGS = 0x100003
CONTEXT_SIZE = 1232
OFF_FLAGS, OFF_EFLAGS, OFF_RCX, OFF_RDX = 0x30, 0x44, 0x80, 0x88
OFF_R8, OFF_R9, OFF_RIP = 0xB0, 0xB8, 0xF8
TRAP_FLAG = 0x100
# x64: PEB.ProcessParameters; RTL_USER_PROCESS_PARAMETERS.CommandLine,
# .Environment, .EnvironmentSize.
PEB_PARAMETERS, PARAMETERS_COMMAND_LINE = 0x20, 0x70
PARAMETERS_ENVIRONMENT, PARAMETERS_ENVIRONMENT_SIZE = 0x80, 0x3F0


class STARTUPINFO(ctypes.Structure):
    _fields_ = [("cb", w.DWORD), ("reserved", w.LPWSTR), ("desktop", w.LPWSTR),
                ("title", w.LPWSTR), ("x", w.DWORD * 7), ("flags", w.DWORD),
                ("show", w.WORD), ("reserved2", w.WORD), ("reserved3", ctypes.c_void_p),
                ("input", w.HANDLE), ("output", w.HANDLE), ("error", w.HANDLE)]


class PROCESS_INFORMATION(ctypes.Structure):
    _fields_ = [("process", w.HANDLE), ("thread", w.HANDLE),
                ("process_id", w.DWORD), ("thread_id", w.DWORD)]


class DEBUG_EVENT(ctypes.Structure):
    _fields_ = [("code", w.DWORD), ("process_id", w.DWORD), ("thread_id", w.DWORD),
                ("pad", w.DWORD), ("u", ctypes.c_ubyte * 160)]


class PROCESS_BASIC_INFORMATION(ctypes.Structure):
    _fields_ = [("exit_status", ctypes.c_void_p), ("peb", ctypes.c_void_p),
                ("affinity", ctypes.c_void_p), ("priority", ctypes.c_void_p),
                ("id", ctypes.c_void_p), ("parent", ctypes.c_void_p)]


k32.GetProcAddress.restype = ctypes.c_void_p
k32.GetProcAddress.argtypes = [w.HMODULE, ctypes.c_char_p]
k32.LoadLibraryW.restype = w.HMODULE
k32.LoadLibraryW.argtypes = [w.LPCWSTR]
k32.ReadProcessMemory.argtypes = [w.HANDLE, ctypes.c_void_p, ctypes.c_void_p,
                                  ctypes.c_size_t, ctypes.POINTER(ctypes.c_size_t)]
k32.WriteProcessMemory.argtypes = [w.HANDLE, ctypes.c_void_p, ctypes.c_void_p,
                                   ctypes.c_size_t, ctypes.c_void_p]
k32.FlushInstructionCache.argtypes = [w.HANDLE, ctypes.c_void_p, ctypes.c_size_t]
k32.GetThreadContext.argtypes = [w.HANDLE, ctypes.c_void_p]
k32.SetThreadContext.argtypes = [w.HANDLE, ctypes.c_void_p]
k32.ContinueDebugEvent.argtypes = [w.DWORD, w.DWORD, w.DWORD]
k32.WaitForDebugEvent.argtypes = [ctypes.POINTER(DEBUG_EVENT), w.DWORD]
k32.OpenProcess.restype = w.HANDLE
k32.OpenProcess.argtypes = [w.DWORD, w.BOOL, w.DWORD]
k32.CloseHandle.argtypes = [w.HANDLE]
k32.DebugActiveProcessStop.argtypes = [w.DWORD]
ntdll.NtQueryInformationProcess.argtypes = [w.HANDLE, ctypes.c_int, ctypes.c_void_p,
                                            w.ULONG, ctypes.c_void_p]

# (module, export, where the name is). The name is: "a"/"w", a NUL-ended
# narrow/wide string in the register; "counted", a wide string in RDX
# (RtlQueryEnvironmentVariable, which has a length in R8 too); "unicode", a
# UNICODE_STRING that RDX points to; None, no name (the whole block).
EXPORTS = [
    ("kernelbase", "GetEnvironmentVariableW", OFF_RCX, "w"),
    ("kernelbase", "GetEnvironmentVariableA", OFF_RCX, "a"),
    ("kernelbase", "ExpandEnvironmentStringsW", OFF_RCX, "w"),
    ("kernelbase", "ExpandEnvironmentStringsA", OFF_RCX, "a"),
    ("kernelbase", "GetEnvironmentStringsW", None, None),
    ("ntdll", "RtlQueryEnvironmentVariable", OFF_RDX, "counted"),
    ("ntdll", "RtlQueryEnvironmentVariable_U", OFF_RDX, "unicode"),
    ("ucrtbase", "getenv", OFF_RCX, "a"),
    ("ucrtbase", "_wgetenv", OFF_RCX, "w"),
    ("ucrtbase", "getenv_s", OFF_R9, "a"),
    ("ucrtbase", "_wgetenv_s", OFF_R9, "w"),
    ("ucrtbase", "_dupenv_s", OFF_R8, "a"),
    ("ucrtbase", "_wdupenv_s", OFF_R8, "w"),
]

# A system DLL is at the same address in every process of this boot, so
# where an export is here is where it is in the debugged ones.
BASES = {}
BREAKPOINTS = {}
for module, export, register, kind in EXPORTS:
    base = BASES.setdefault(module, k32.LoadLibraryW(module))
    address = k32.GetProcAddress(base, export.encode())
    if address:
        BREAKPOINTS[address] = (module, export, register, kind)


def read(process, address, size):
    buffer = ctypes.create_string_buffer(size)
    got = ctypes.c_size_t()
    if not address or not k32.ReadProcessMemory(process, address, buffer, size, ctypes.byref(got)):
        return None
    return buffer.raw[: got.value]


def write(process, address, data):
    k32.WriteProcessMemory(process, address, data, len(data), None)
    k32.FlushInstructionCache(process, address, len(data))


def pointer_at(process, address):
    data = read(process, address, 8)
    return struct.unpack("<Q", data)[0] if data and len(data) == 8 else 0


def string_at(process, address, wide, length=None):
    """The string at `address`: `length` units, or up to its NUL."""
    unit = 2 if wide else 1
    if length is not None:
        data = read(process, address, min(length, 1024) * unit) or b""
    else:
        data = b""
        while len(data) < 1024 * unit:
            chunk = read(process, address + len(data), 64 * unit)
            if not chunk:
                break
            data += chunk
            ends = [i for i in range(0, len(data), unit) if data[i:i + unit] == b"\0" * unit]
            if ends:
                data = data[: ends[0]]
                break
    return data.decode("utf-16-le" if wide else "latin-1", "replace")


def peb_of(process):
    information = PROCESS_BASIC_INFORMATION()
    status = ntdll.NtQueryInformationProcess(process, 0, ctypes.byref(information),
                                             ctypes.sizeof(information), None)
    return information.peb if status == 0 else 0


def command_line_of(process):
    parameters = pointer_at(process, peb_of(process) + PEB_PARAMETERS)
    counted = read(process, parameters + PARAMETERS_COMMAND_LINE, 16)
    if not counted or len(counted) < 16:
        return ""
    length = struct.unpack_from("<H", counted, 0)[0]
    return string_at(process, struct.unpack_from("<Q", counted, 8)[0], True, length // 2)


def program_of(command_line):
    """The first word of a command line, as the C runtime splits it."""
    if command_line.startswith('"'):
        return command_line[1:].split('"', 1)[0]
    return command_line.split(" ", 1)[0]


def environment_from_outside(process_id):
    """The names in process `process_id`'s environment, read as a test
    would, or why they couldn't be."""
    process = k32.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, False, process_id)
    if not process:
        return f"OpenProcess failed ({ctypes.get_last_error()})"
    try:
        parameters = pointer_at(process, peb_of(process) + PEB_PARAMETERS)
        block = pointer_at(process, parameters + PARAMETERS_ENVIRONMENT)
        size = pointer_at(process, parameters + PARAMETERS_ENVIRONMENT_SIZE)
        data = read(process, block, size) if block and 0 < size < 1 << 20 else None
        if not data:
            return f"ReadProcessMemory failed ({ctypes.get_last_error()})"
        # The block ends at its first empty string: what follows is left
        # over (a variable the process removed, moved over).
        strings = data.decode("utf-16-le", "replace").split("\0")
        entries = itertools.takewhile(bool, strings)
        return sorted(entry.split("=", 1)[0] for entry in entries if not entry.startswith("="))
    finally:
        k32.CloseHandle(process)


class Context:
    """A thread's CONTEXT, in a buffer aligned as the system requires."""

    def __init__(self, thread):
        self.storage = ctypes.create_string_buffer(CONTEXT_SIZE + 16)
        self.address = (ctypes.addressof(self.storage) + 15) & ~15
        self.view = (ctypes.c_ubyte * CONTEXT_SIZE).from_address(self.address)
        struct.pack_into("<I", self.view, OFF_FLAGS, CONTEXT_FLAGS)
        self.thread = thread
        self.ok = bool(k32.GetThreadContext(thread, self.address))

    def get(self, offset):
        return struct.unpack_from("<Q", self.view, offset)[0]

    def set(self, offset, value):
        struct.pack_into("<Q", self.view, offset, value)

    def save(self):
        k32.SetThreadContext(self.thread, self.address)


class Process:
    def __init__(self, handle, index, command_line):
        self.handle = handle
        self.index = index
        words = command_line.split()
        self.kind = ("renderer" if "--type=renderer" in words
                     else "probe" if "--type=renderer-sandbox-probe" in words else "other")
        self.threads = {}
        self.originals = {}
        # Breakpoints taken out while a thread steps over them.
        self.out = set()
        self.asked = collections.defaultdict(set)
        self.outside = None
        self.exit_code = None

    def arm(self, module):
        for address, (owner, *_) in BREAKPOINTS.items():
            if owner == module and address not in self.originals:
                original = read(self.handle, address, 1)
                if original:
                    self.originals[address] = original
                    write(self.handle, address, b"\xcc")

    def name_asked(self, context, register, kind):
        if kind is None:
            return None
        if kind in ("a", "w"):
            return string_at(self.handle, context.get(register), kind == "w")
        if kind == "counted":
            # The length in R8 isn't the name's for every caller (0, or one
            # short of it); the name is NUL-ended for all seen.
            return string_at(self.handle, context.get(OFF_RDX), True)
        counted = read(self.handle, context.get(OFF_RDX), 16)
        if not counted or len(counted) < 16:
            return None
        length = struct.unpack_from("<H", counted, 0)[0]
        return string_at(self.handle, struct.unpack_from("<Q", counted, 8)[0], True, length // 2)


def trace(command):
    program = shutil.which(command[0]) or command[0]
    line = " ".join(f'"{word}"' for word in [program, *command[1:]])
    startup = STARTUPINFO()
    startup.cb = ctypes.sizeof(startup)
    started = PROCESS_INFORMATION()
    if not k32.CreateProcessW(program, line, None, None, False, DEBUG_PROCESS, None, None,
                              ctypes.byref(startup), ctypes.byref(started)):
        raise ctypes.WinError(ctypes.get_last_error())
    processes, stepping, loaded = {}, {}, set()
    alive = 0
    event = DEBUG_EVENT()
    while True:
        if not k32.WaitForDebugEvent(ctypes.byref(event), INFINITE):
            raise ctypes.WinError(ctypes.get_last_error())
        status = DBG_CONTINUE
        data = bytes(event.u)
        process = processes.get(event.process_id)
        if event.code == 3:  # CREATE_PROCESS_DEBUG_EVENT
            _file, handle, thread = struct.unpack_from("<QQQ", data, 0)
            command_line = command_line_of(handle)
            if processes and "machopo" not in program_of(command_line).lower():
                # Not machopo (a test's PowerShell): let go of it, it is
                # slow to debug and nothing to measure.
                k32.ContinueDebugEvent(event.process_id, event.thread_id, DBG_CONTINUE)
                k32.DebugActiveProcessStop(event.process_id)
                continue
            process = Process(handle, len(processes), command_line)
            process.threads[event.thread_id] = thread
            processes[event.process_id] = process
            alive += 1
            process.arm("ntdll")
            if len(processes) % 50 == 0:
                print(f"getenv_trace: {len(processes)} processes", file=sys.stderr, flush=True)
        elif event.code == 2:  # CREATE_THREAD_DEBUG_EVENT
            process.threads[event.thread_id] = struct.unpack_from("<Q", data, 0)[0]
            # Its threads start once it has lowered itself; the last of
            # these reads is the one that counts.
            if process.kind == "renderer" and len(process.threads) > 4:
                process.outside = environment_from_outside(event.process_id)
        elif event.code == 4:  # EXIT_THREAD_DEBUG_EVENT
            process.threads.pop(event.thread_id, None)
        elif event.code == 6:  # LOAD_DLL_DEBUG_EVENT
            base = struct.unpack_from("<Q", data, 8)[0]
            for module, module_base in BASES.items():
                if module_base == base:
                    process.arm(module)
        elif event.code == 5:  # EXIT_PROCESS_DEBUG_EVENT
            process.exit_code = struct.unpack_from("<I", data, 0)[0]
            alive -= 1
        elif event.code == 1:  # EXCEPTION_DEBUG_EVENT
            code = struct.unpack_from("<I", data, 0)[0]
            address = struct.unpack_from("<Q", data, 16)[0]
            if code == EXCEPTION_BREAKPOINT and address in process.originals:
                thread = process.threads[event.thread_id]
                context = Context(thread)
                module, export, register, kind = BREAKPOINTS[address]
                process.asked[process.name_asked(context, register, kind)].add(export)
                # Run the original instruction: with the breakpoint out for
                # one step, put back after it. Other threads aren't held
                # (holding them deadlocks when two hit at once), so one may
                # pass meanwhile unseen: a name read only then is missed.
                # One that hit it before it was taken out is handled as
                # this one, without a step of its own.
                context.set(OFF_RIP, address)
                if address not in process.out:
                    write(process.handle, address, process.originals[address])
                    process.out.add(address)
                    eflags = struct.unpack_from("<I", context.view, OFF_EFLAGS)[0]
                    struct.pack_into("<I", context.view, OFF_EFLAGS, eflags | TRAP_FLAG)
                    stepping[event.thread_id] = address
                context.save()
            elif code == EXCEPTION_SINGLE_STEP and event.thread_id in stepping:
                address = stepping.pop(event.thread_id)
                write(process.handle, address, b"\xcc")
                process.out.discard(address)
            elif code == EXCEPTION_BREAKPOINT and event.process_id not in loaded:
                loaded.add(event.process_id)  # the loader's, once a process
            else:
                status = DBG_EXCEPTION_NOT_HANDLED
        k32.ContinueDebugEvent(event.process_id, event.thread_id, status)
        if event.code == 5 and alive == 0:
            return processes


def report(processes):
    lines = []
    for kind in ("renderer", "probe", "other"):
        group = [p for p in processes.values() if p.kind == kind]
        lines.append(f"== {kind} processes: {len(group)}")
        names = collections.defaultdict(lambda: [0, set()])
        for process in group:
            for name, exports in process.asked.items():
                names[name][0] += 1
                names[name][1] |= exports
        for name in sorted(names, key=lambda n: (n is None, (n or "").lower())):
            count, exports = names[name]
            shown = "(the whole block)" if name is None else (
                name if name.isprintable() and name.isascii() else ascii(name))
            lines.append(f"  {shown:44} {count:4} processes  {', '.join(sorted(exports))}")
        if kind == "renderer":
            outside = [p.outside for p in group]
            read = [o for o in outside if isinstance(o, list)]
            failed = sorted({o for o in outside if isinstance(o, str)})
            lines.append(f"  environment read from outside: {len(read)} of {len(group)}; "
                         f"failed: {failed or 'none'}; not tried (too few threads): "
                         f"{outside.count(None)}")
            # Each different environment once, smallest first: a limited
            # renderer's is short, one the test started itself has it all.
            for names in sorted({tuple(o) for o in read}, key=len):
                count = sum(1 for o in read if tuple(o) == names)
                lines.append(f"  {count} with {len(names)} names: {' '.join(names)[:400]}")
            exits = collections.Counter(p.exit_code for p in group)
            lines.append(f"  exit codes: {sorted(exits.items(), key=str)}")
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", help="write the report here as well")
    parser.add_argument("command", nargs=argparse.REMAINDER)
    arguments = parser.parse_args()
    command = arguments.command[1:] if arguments.command[:1] == ["--"] else arguments.command
    if not command:
        parser.error("no command")
    processes = trace(command)
    text = report(processes)
    sys.stdout.reconfigure(encoding="utf-8", errors="backslashreplace")
    sys.stdout.write(text)
    if arguments.out:
        with open(arguments.out, "w", encoding="utf-8", newline="\n") as out:
            out.write(text)
    first = min(processes.values(), key=lambda p: p.index)
    sys.exit(first.exit_code or 0)


if __name__ == "__main__":
    main()
