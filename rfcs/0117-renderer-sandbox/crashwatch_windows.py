#!/usr/bin/env python3
"""Says where a renderer process ended itself on Windows, on a machine with no
debugger: waits for a child of the process PARENT_PID named machopo.exe,
attaches to it as its debugger, and prints the stack of the thread that
takes an exception nothing handles (a fast fail, which `abort` is), or what
the process exited with.

    crashwatch_windows.py PARENT_PID

`probe_windows.py` starts it for its machopo when `PROBE_WATCH` in the
environment is this file's path. The renderer has to still be starting when
this attaches: the probe's prototype build (not kept) waited three seconds
at a renderer's start for that; machopo as it is doesn't, and a renderer
that ends at once is gone before this finds it.
"""
import ctypes
import ctypes.wintypes as wt
import os
import sys
import time

# Where machopo.pdb is.
SYMBOLS = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)),
                                        "..", "..", "target", "debug"))

k32 = ctypes.WinDLL("kernel32", use_last_error=True)
dbg = ctypes.WinDLL("dbghelp", use_last_error=True)
k32.OpenProcess.restype = wt.HANDLE
k32.OpenThread.restype = wt.HANDLE
k32.CreateToolhelp32Snapshot.restype = wt.HANDLE


class PE(ctypes.Structure):
    _fields_ = [("dwSize", wt.DWORD), ("cntUsage", wt.DWORD), ("pid", wt.DWORD),
                ("heap", ctypes.c_size_t), ("mod", wt.DWORD), ("threads", wt.DWORD),
                ("ppid", wt.DWORD), ("pri", wt.LONG), ("flags", wt.DWORD), ("exe", wt.WCHAR * 260)]


def child_of(parent):
    snap = wt.HANDLE(k32.CreateToolhelp32Snapshot(2, 0))
    e = PE()
    e.dwSize = ctypes.sizeof(e)
    ok = k32.Process32FirstW(snap, ctypes.byref(e))
    found = None
    while ok:
        if e.ppid == parent and e.exe.lower() == "machopo.exe":
            found = e.pid
        ok = k32.Process32NextW(snap, ctypes.byref(e))
    k32.CloseHandle(snap)
    return found


class EXCEPTION_RECORD(ctypes.Structure):
    _fields_ = [("code", wt.DWORD), ("flags", wt.DWORD), ("record", ctypes.c_void_p),
                ("address", ctypes.c_void_p), ("nparams", wt.DWORD),
                ("info", ctypes.c_uint64 * 15)]


class EXCEPTION_DEBUG_INFO(ctypes.Structure):
    _fields_ = [("record", EXCEPTION_RECORD), ("first", wt.DWORD)]


class U(ctypes.Union):
    _fields_ = [("exception", EXCEPTION_DEBUG_INFO), ("exit_code", wt.DWORD), ("raw", ctypes.c_byte * 160)]


class DEBUG_EVENT(ctypes.Structure):
    _fields_ = [("code", wt.DWORD), ("pid", wt.DWORD), ("tid", wt.DWORD), ("u", U)]


class ADDRESS64(ctypes.Structure):
    _fields_ = [("offset", ctypes.c_uint64), ("segment", wt.WORD), ("mode", ctypes.c_int)]


class STACKFRAME64(ctypes.Structure):
    _fields_ = [("pc", ADDRESS64), ("ret", ADDRESS64), ("frame", ADDRESS64), ("stack", ADDRESS64),
                ("bstore", ADDRESS64), ("table", ctypes.c_void_p), ("params", ctypes.c_uint64 * 4),
                ("far", wt.BOOL), ("virtual", wt.BOOL), ("reserved", ctypes.c_uint64 * 3),
                ("kd", ctypes.c_byte * 96)]


class SYMBOL_INFO(ctypes.Structure):
    _fields_ = [("size", wt.ULONG), ("type", wt.ULONG), ("r", ctypes.c_uint64 * 2), ("index", wt.ULONG),
                ("sz", wt.ULONG), ("modbase", ctypes.c_uint64), ("flags", wt.ULONG),
                ("value", ctypes.c_uint64), ("address", ctypes.c_uint64), ("register", wt.ULONG),
                ("scope", wt.ULONG), ("tag", wt.ULONG), ("namelen", wt.ULONG), ("maxname", wt.ULONG),
                ("name", ctypes.c_char * 512)]


dbg.SymFunctionTableAccess64.restype = ctypes.c_void_p
dbg.SymFunctionTableAccess64.argtypes = [wt.HANDLE, ctypes.c_uint64]
dbg.SymGetModuleBase64.restype = ctypes.c_uint64
dbg.SymGetModuleBase64.argtypes = [wt.HANDLE, ctypes.c_uint64]
dbg.SymFromAddr.argtypes = [wt.HANDLE, ctypes.c_uint64, ctypes.POINTER(ctypes.c_uint64),
                            ctypes.POINTER(SYMBOL_INFO)]


def stack(process, tid):
    thread = wt.HANDLE(k32.OpenThread(0x1FFFFF, False, tid))
    raw = ctypes.create_string_buffer(1232 + 16)
    base = ctypes.addressof(raw)
    context = (base + 15) & ~15
    ctypes.c_uint32.from_address(context + 0x30).value = 0x10000B  # CONTEXT_FULL
    if not k32.GetThreadContext(thread, ctypes.c_void_p(context)):
        print("  no context", ctypes.get_last_error())
        return
    frame = STACKFRAME64()
    frame.pc.offset = ctypes.c_uint64.from_address(context + 0xF8).value
    frame.stack.offset = ctypes.c_uint64.from_address(context + 0x98).value
    frame.frame.offset = ctypes.c_uint64.from_address(context + 0xA0).value
    frame.pc.mode = frame.stack.mode = frame.frame.mode = 3
    for _ in range(45):
        if not dbg.StackWalk64(0x8664, process, thread, ctypes.byref(frame), ctypes.c_void_p(context),
                               None, dbg.SymFunctionTableAccess64, dbg.SymGetModuleBase64, None):
            break
        symbol = SYMBOL_INFO()
        symbol.size = 88
        symbol.maxname = 500
        displacement = ctypes.c_uint64()
        if dbg.SymFromAddr(process, frame.pc.offset, ctypes.byref(displacement), ctypes.byref(symbol)):
            print(f"  {symbol.name.decode(errors='replace')[:150]}+{displacement.value:#x}")
        else:
            print(f"  {frame.pc.offset:#x}")


def main():
    parent = int(sys.argv[1])
    deadline = time.time() + 30
    pid = None
    while pid is None and time.time() < deadline:
        pid = child_of(parent)
        time.sleep(0.01)
    if pid is None:
        raise SystemExit("no renderer")
    if not k32.DebugActiveProcess(pid):
        raise SystemExit(f"not attached: {ctypes.get_last_error()}")
    process = wt.HANDLE(k32.OpenProcess(0x1FFFFF, False, pid))
    dbg.SymSetOptions(0x00000002 | 0x00000004 | 0x00000010)  # UNDNAME | DEFERRED_LOADS | LOAD_LINES
    dbg.SymInitializeW(process, SYMBOLS, False)
    event = DEBUG_EVENT()
    first_breakpoint = True
    while k32.WaitForDebugEvent(ctypes.byref(event), 30000):
        status = 0x00010002  # DBG_CONTINUE
        if event.code == 1:
            record = event.u.exception.record
            if record.code == 0x80000003 and first_breakpoint:
                first_breakpoint = False
            elif event.u.exception.first and record.code != 0xC0000409:
                print(f"first-chance {record.code:#x} at {record.address:#x} (tid {event.tid})")
                if record.code == 0xC0000005:
                    dbg.SymRefreshModuleList(process)
                    stack(process, event.tid)
                status = 0x80010001  # DBG_EXCEPTION_NOT_HANDLED
            else:
                print(f"exception {record.code:#x} at {record.address:#x} (tid {event.tid}) "
                      f"params {[hex(x) for x in record.info[:record.nparams]]}")
                dbg.SymRefreshModuleList(process)
                stack(process, event.tid)
                status = 0x80010001
        elif event.code == 5:
            print(f"exit {event.u.exit_code:#x}")
            k32.ContinueDebugEvent(event.pid, event.tid, status)
            return
        k32.ContinueDebugEvent(event.pid, event.tid, status)
    print("no more events")


main()
