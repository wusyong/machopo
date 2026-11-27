"""Debug a command and its children: DLL loads, exceptions, debug strings
(with loader snaps on), exit codes. Scratch tool for the lockdown probe."""

import ctypes
import ctypes.wintypes as w
import struct
import sys

k32 = ctypes.WinDLL("kernel32", use_last_error=True)
ntdll = ctypes.WinDLL("ntdll")

DEBUG_PROCESS = 0x1
DBG_CONTINUE = 0x00010002
DBG_EXCEPTION_NOT_HANDLED = 0x80010001


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


class PBI(ctypes.Structure):
    _fields_ = [("exit_status", ctypes.c_void_p), ("peb", ctypes.c_void_p),
                ("affinity", ctypes.c_void_p), ("priority", ctypes.c_void_p),
                ("id", ctypes.c_void_p), ("parent", ctypes.c_void_p)]


k32.ReadProcessMemory.argtypes = [w.HANDLE, ctypes.c_void_p, ctypes.c_void_p,
                                  ctypes.c_size_t, ctypes.POINTER(ctypes.c_size_t)]
k32.WriteProcessMemory.argtypes = [w.HANDLE, ctypes.c_void_p, ctypes.c_void_p,
                                   ctypes.c_size_t, ctypes.c_void_p]
k32.ContinueDebugEvent.argtypes = [w.DWORD, w.DWORD, w.DWORD]
k32.WaitForDebugEvent.argtypes = [ctypes.POINTER(DEBUG_EVENT), w.DWORD]
k32.GetFinalPathNameByHandleW.argtypes = [w.HANDLE, w.LPWSTR, w.DWORD, w.DWORD]
ntdll.NtQueryInformationProcess.argtypes = [w.HANDLE, ctypes.c_int, ctypes.c_void_p,
                                            w.ULONG, ctypes.c_void_p]


def read(process, address, size):
    buffer = ctypes.create_string_buffer(size)
    got = ctypes.c_size_t()
    if not address or not k32.ReadProcessMemory(process, address, buffer, size, ctypes.byref(got)):
        return None
    return buffer.raw[:got.value]


def path_of(handle):
    if not handle:
        return "?"
    buffer = ctypes.create_unicode_buffer(1024)
    n = k32.GetFinalPathNameByHandleW(handle, buffer, 1024, 0)
    return buffer.value if n else "?"


def snaps_on(process):
    info = PBI()
    ntdll.NtQueryInformationProcess(process, 0, ctypes.byref(info), ctypes.sizeof(info), None)
    flag = read(process, info.peb + 0xBC, 4)
    if flag:
        value = struct.unpack("<I", flag)[0] | 0x2
        k32.WriteProcessMemory(process, info.peb + 0xBC, struct.pack("<I", value), 4, None)


def main():
    snaps = sys.argv[1] == "--snaps"
    command = sys.argv[2:] if snaps else sys.argv[1:]
    line = " ".join(f'"{word}"' for word in command)
    startup = STARTUPINFO()
    startup.cb = ctypes.sizeof(startup)
    started = PROCESS_INFORMATION()
    if not k32.CreateProcessW(command[0], line, None, None, False, DEBUG_PROCESS, None, None,
                              ctypes.byref(startup), ctypes.byref(started)):
        raise ctypes.WinError(ctypes.get_last_error())
    handles, index, alive = {}, {}, 0
    event = DEBUG_EVENT()
    while True:
        k32.WaitForDebugEvent(ctypes.byref(event), 0xFFFFFFFF)
        data = bytes(event.u)
        status = DBG_CONTINUE
        p = index.setdefault(event.process_id, len(index))
        if event.code == 3:
            file, handle = struct.unpack_from("<QQ", data, 0)
            handles[event.process_id] = handle
            alive += 1
            print(f"[{p}] start {path_of(file)}")
            if snaps and p > 0:
                snaps_on(handle)
        elif event.code == 6:
            file, base = struct.unpack_from("<QQ", data, 0)
            if p > 0:
                print(f"[{p}] load {path_of(file)}")
        elif event.code == 8:  # OUTPUT_DEBUG_STRING
            address, unicode, length = struct.unpack_from("<QHH", data, 0)
            raw = read(handles[event.process_id], address, length * (2 if unicode else 1))
            if raw and p > 0:
                text = raw.decode("utf-16-le" if unicode else "mbcs", "replace").rstrip("\0\n")
                if any(k in text for k in ("ERROR", "WARNING", "failed", "Failed", "status 0xc")):
                    print(f"[{p}] dbg {text}")
        elif event.code == 1:
            code, flags, record, address = struct.unpack_from("<IIQQ", data, 0)
            first = struct.unpack_from("<I", data, 152)[0]
            if code not in (0x80000003, 0x406D1388):
                print(f"[{p}] exception {code:08X} at {address:X} first={first}")
                status = DBG_EXCEPTION_NOT_HANDLED
        elif event.code == 5:
            print(f"[{p}] exit {struct.unpack_from('<I', data, 0)[0]:08X}")
            alive -= 1
        k32.ContinueDebugEvent(event.process_id, event.thread_id, status)
        if event.code == 5 and alive == 0:
            return


main()
