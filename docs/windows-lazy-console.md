# Avoiding unnecessary console windows on Windows

**Status:** design. Nothing here is implemented yet.
**Scope:** Windows shims only. Linux shims `execv` into the target and never
deal with consoles.

## The problem

When a console program starts from a process that has no console, Windows
creates a console window for it before any of its code runs. Common cases:

- double-clicking a `.py` file or a shortcut in Explorer
- Task Scheduler jobs
- GUI applications that start a console program without special flags

If the program finishes quickly, the window flashes. If it never prints
anything, the window is empty for as long as it runs.

`python.exe` behaves this way today, with or without pyenv. The rpyenv shim
has two jobs:

1. **Never make it worse.** The shim must not add a window that running the
   target directly would not create.
2. **Make it better where possible.** When there is no console, show a window
   only once the program actually prints something. If it never prints, no
   window appears.

## Background: how Windows decides

### Program type

Every `.exe` is either a console program or a GUI program. This is set in its
header (the PE `Subsystem` field):

| Program type | Shell (cmd, PowerShell) waits for it | Gets a console automatically |
|---|---|---|
| Console (CUI), e.g. `python.exe` | Yes | Yes, unless it inherits one |
| GUI, e.g. `pythonw.exe` | No | No |

A shim for a console command has to be a console program. Otherwise cmd and
PowerShell would return to the prompt immediately instead of waiting for it.

### What the caller controls

When a process starts a console program, its creation flags decide what the
child gets:

| Caller situation | Child gets |
|---|---|
| Caller has a console, no flags | The caller's console, shared |
| Caller has no console, no flags | A new console with a visible window |
| `CREATE_NEW_CONSOLE` | A new console with a visible window |
| `CREATE_NO_WINDOW` | A new console with no window |
| `DETACHED_PROCESS` | No console |

A caller that doesn't want a window is expected to pass `CREATE_NO_WINDOW`.
VS Code and Python's own `subprocess` with `creationflags` do this. The shim
must respect it.

### Console allocation policy (Windows 11 24H2+)

Windows 11 24H2 and Windows Server 2025 (build 26100) add an application
manifest setting that separates "the shell waits for me" from "I get a console
automatically":

```xml
<consoleAllocationPolicy xmlns="http://schemas.microsoft.com/SMI/2024/WindowsSettings">detached</consoleAllocationPolicy>
```

A console program with `detached`:

- still inherits its parent's console, so it behaves normally in a terminal,
- still makes cmd and PowerShell wait for it,
- **does not** get a console created for it when there is none to inherit.

The program can create one later with `AllocConsoleWithOptions`. With
`ALLOC_CONSOLE_MODE_DEFAULT`, that function allocates "a console session if
(and how) one was requested by the parent process". In other words, it
creates the console the process would have received at startup: a window, a
windowless console, or none. It just happens later.

Both the manifest setting and `AllocConsoleWithOptions` are exported and
documented for build 26100 and later. Older Windows versions ignore the
manifest setting, so the shim still gets a console at startup as usual.

### Default terminal

On Windows 11 the default terminal can be Windows Terminal instead of the
classic console window (conhost). When it is, a new console opens as a
Windows Terminal tab. `GetConsoleWindow` then returns a placeholder window
handle, and calling `ShowWindow` on it does nothing useful. The design below
avoids depending on which terminal is the default.

## Design

### Rule 1: the shim type matches the target's type

`pyenv rehash` reads the PE `Subsystem` field of each target executable (for
example the entries in `versions/*/Scripts`) and picks the matching shim:

- console target → `pyenv-shim.exe` (console program)
- GUI target → `pyenv-shimw.exe` (GUI program)

This covers `pythonw.exe` and pip's GUI-script launchers (`w64.exe`)
automatically. It needs no list of names. If the same command is a console
program in one Python version and a GUI program in another, use the console
shim.

### Rule 2: the child ends up with the console the shim has

The console shim never passes `CREATE_NEW_CONSOLE`. It decides how to start
the child as follows:

```
shim starts
├─ attached to a console? ─────────────── yes → INHERIT
└─ no
   ├─ stdout and stderr both redirected? ─ yes → NO-WINDOW
   ├─ 24H2 APIs unavailable? ──────────── yes → MIRROR
   ├─ RPYENV_CONSOLE=eager, or
   │  only some handles redirected? ────── yes → EAGER
   └─ otherwise ─────────────────────────────── → LAZY
```

| Mode | Child is started with | Window appears |
|---|---|---|
| INHERIT | The shim's console and std handles, inherited | Whatever the caller arranged. This is the normal case in a terminal. |
| NO-WINDOW | `CREATE_NO_WINDOW`; the caller's redirected handles; stdin set to `NUL` if the caller didn't provide it | Never |
| MIRROR | `DETACHED_PROCESS` | Never. Same as `python.exe` started with that flag. |
| EAGER | `AllocConsoleWithOptions(DEFAULT)` at startup, then INHERIT | At startup, exactly as with `python.exe` |
| LAZY | A pseudo-console (ConPTY), relayed by the shim | On the first printable output (see below) |

How the shim checks each condition:

- **Attached to a console:** `GetConsoleProcessList` returns a non-zero count.
- **Redirected:** `GetStdHandle` returns a non-null handle whose `GetFileType`
  is `FILE_TYPE_DISK` or `FILE_TYPE_PIPE`. These are the shim's own handles,
  with no I/O pending, so `GetFileType` can't block.
- **24H2 APIs available:** `AllocConsoleWithOptions` resolves from
  `kernel32.dll` at runtime.

Why NO-WINDOW uses `CREATE_NO_WINDOW` rather than `DETACHED_PROCESS`: with a
windowless console, processes that Python starts inherit that console. With
no console at all, each of those processes would get a new window of its own.

On Windows versions before 24H2, the shim gets a console at startup (INHERIT),
unless the caller passed `DETACHED_PROCESS` (MIRROR). That is exactly
`python.exe`'s behavior, so rule 1 ("never make it worse") holds on every
version.

The shim's manifest includes `consoleAllocationPolicy=detached`. This is safe
on all versions, because older Windows ignores the setting.

### LAZY mode: pseudo-console relay

1. **Create the pseudo-console.** Create two pipes and call
   `CreatePseudoConsole` with an initial size. Start the child with
   `PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE`, inside the shim's Job Object (the
   same one that kills the child if the shim dies).
2. **Always drain the output.** A dedicated thread reads the pseudo-console's
   output pipe continuously. If the pipe fills up, the child blocks. Until the
   window is shown, the bytes are buffered in memory.
3. **Watch for the trigger.** Scan the output as a VT stream:
   - skip escape sequences (CSI `ESC [ … final`, OSC `ESC ] … BEL|ST`,
     DCS/SOS/PM/APC `… ST`, two-byte `ESC x`),
   - skip C0 control characters and whitespace.

   The first character left over is the trigger. ConPTY emits its own setup
   sequences (modes, title, cursor), so "first byte" would fire immediately.
   Capture ConPTY's actual startup output during implementation and use it as
   a test fixture.
4. **Show the window.** Call `AllocConsoleWithOptions` with
   `ALLOC_CONSOLE_MODE_DEFAULT`. This gives the console the caller would have
   given `python.exe`, so a `CREATE_NO_WINDOW` caller still gets no window.
   Then:
   - output mode: `ENABLE_VIRTUAL_TERMINAL_PROCESSING`,
     `DISABLE_NEWLINE_AUTO_RETURN`
   - input mode: `ENABLE_VIRTUAL_TERMINAL_INPUT`, with processed, line, and
     echo input turned off, so keys, including Ctrl+C as `0x03`, pass through
     as raw bytes
   - `ResizePseudoConsole` to the new window's size
   - write the buffered output, then keep relaying
   - set the window title to the command name
5. **Relay input.** An input thread calls `ReadConsoleInputW`. It forwards key
   characters (VT sequences, because of VT input mode) to the pseudo-console's
   input pipe. It turns `WINDOW_BUFFER_SIZE_EVENT` into `ResizePseudoConsole`.
   ConPTY converts `0x03` into a `CTRL_C_EVENT` for the child. The shim itself
   ignores Ctrl+C.
6. **Shut down.** When the child exits, keep reading the output pipe until it
   closes, and call `ClosePseudoConsole` from a thread other than the reader.
   Before 24H2, `ClosePseudoConsole` waits until the output is drained. The
   shim then returns the child's exit code.
   - **User closes the window:** the shim gets `CTRL_CLOSE_EVENT` and calls
     `ClosePseudoConsole`, which sends `CTRL_CLOSE_EVENT` to the child. The
     Job Object cleans up anything left.
   - **Non-zero exit after the shim opened a new window**
     (`ALLOC_CONSOLE_RESULT_NEW_CONSOLE`): print the exit code and wait for a
     key before closing, so a traceback from a double-clicked script stays
     readable. In INHERIT mode this never happens, because the terminal belongs
     to the caller.

What the child sees in LAZY mode:

- **A real console:** `isatty()` is true, console APIs work, colors work,
  and the REPL works.
- **No extra windows for processes it starts:** they share the pseudo-console.
- **A fixed size until the window appears.** Programs that check the width at
  startup (e.g. argparse help) format for the initial size.
- **Re-rendered output.** ConPTY re-renders output instead of passing the raw
  bytes through. On screen the result looks the same.

If any ConPTY or `AllocConsoleWithOptions` call fails, the shim falls back to
EAGER.

### Configuration

`RPYENV_CONSOLE` selects the behavior when the shim has no console:

- `lazy`: as described above. This is the intended default when the 24H2 APIs
  are available, but it is decided only after the cost is measured (open
  question 3).
- `eager`: behave like `python.exe` and create the console at startup

This setting is specific to rpyenv; upstream pyenv has no equivalent. The
`RPYENV_` prefix keeps rpyenv-only settings from colliding with any future
`PYENV_` variable.

## Deferred: detecting input without prior output

In LAZY mode, a program that reads from the console **without printing
anything first** waits with no window visible. We decided not to handle this
for now. A Python script, or any program it starts, that asks for input
normally prints a prompt first, and that output shows the window. Revisit this
if a real case shows up. Until then, `RPYENV_CONSOLE=eager` is the workaround.

A probe already showed it can be done without injecting code into the child.
It is recorded here so it doesn't have to be worked out again.

**Rule.** For each thread of each process in the Job Object, call
`NtQueryInformationThread(ThreadLastSystemCall)`. This is info class 21. It is
undocumented, and System Informer relies on it. It returns the thread's
current system call number and first argument. The thread is waiting for
console input when either:

- the call is `NtDeviceIoControlFile` and the IOCTL code has device type
  `0x50` (`FILE_DEVICE_CONSOLE`). The code is the 6th argument; on x64, read
  it at user RSP + 0x30 via `GetThreadContext` and `ReadProcessMemory`. The
  observed value was `0x00500016`.
- the call is `NtReadFile` or `NtWaitForSingleObject` on the process's
  standard input handle. Read that handle from the PEB:
  `ProcessParameters` + 0x20.

Look up syscall numbers at runtime from the shim's own `ntdll` stubs. On x64
each stub starts with the bytes `4C 8B D1 B8`, followed by the number as a
32-bit value.

**Result.** 14 of 14 scenarios were classified correctly. Method: Windows 11
build 26200, x64. Each child ran in a windowless conhost console and was
sampled after 4 s.

- **Detected as waiting (9):**
  - Python: `input()` without a prompt, `sys.stdin.read()`,
    `msvcrt.getwch()`, `getpass()`, REPL
  - `cmd set /p`, `findstr` reading stdin, `choice.exe`
  - PowerShell `Read-Host`
- **Correctly not detected (5):**
  - `time.sleep`, `threading.Event.wait`
  - a blocked pipe read, a blocked socket `accept`
  - print, then sleep

**Known limits:**

- **The value is stale while a thread is running.** Only trust it once the
  process's CPU time has stopped advancing between polls.
- **Don't identify handles by querying them.** A query on a synchronous handle
  that has a read pending can block until the read finishes.
- **Untested:** under ConPTY, on ARM64, for grandchild processes, and for
  waits on stdin through `WaitForMultipleObjects`.

## Testing

- **Unit tests:**
  - the mode choice as a pure function of (attached, which handles are
    redirected, APIs available, `RPYENV_CONSOLE`), tested against a table
    that mirrors the decision tree
  - the trigger scanner, with captured ConPTY startup output and mixed
    escape/text fixtures
- **Integration tests on GitHub Actions `windows-2025`** (Windows Server 2025,
  build 26100, which meets the 24H2 requirement):
  - To simulate an Explorer launch, the test harness must have no console
    itself. Start a helper with `DETACHED_PROCESS`, and have the helper start
    the shim with no flags.
  - The shim writes its decisions to a debug log (mode, allocation result,
    time of reveal). Tests assert on that log.
  - Check "no window yet" **while the child is still running**: the test
    script sleeps, then prints, then sleeps again. Checking after it exits
    tells nothing, because the console is gone either way.
- **Manual checklist (the harness can't reproduce these):**
  - Explorer double-click, a `.lnk` shortcut, a Task Scheduler job, and cmd's
    `start`
  - each with Windows Terminal and with conhost as the default terminal

## Open questions

1. **Explorer launches with `ALLOC_CONSOLE_MODE_DEFAULT`.** The docs imply a
   launch with no flags and no console gets a visible window. Confirm on a real
   Explorer launch.
2. **Initial pseudo-console size.** Use a fixed 120×30, or read the default
   console size from `HKCU\Console`?
3. **Cost of ConPTY startup.** LAZY mode starts an extra headless conhost for
   each launch. Measure it before making `lazy` the default.
4. **Hold on error.** Should keeping the window open after a failure be
   configurable, or always on?

## References

- [Console Allocation Policy](https://learn.microsoft.com/en-us/windows/console/console-allocation-policy)
- [AllocConsoleWithOptions](https://learn.microsoft.com/en-us/windows/console/allocconsolewithoptions)
  and [ALLOC_CONSOLE_OPTIONS](https://learn.microsoft.com/en-us/windows/console/alloc-console-options)
- [Spec #7335: Console Allocation Policy](https://github.com/microsoft/terminal/blob/main/doc/specs/%237335%20-%20Console%20Allocation%20Policy.md)
- [ClosePseudoConsole](https://learn.microsoft.com/en-us/windows/console/closepseudoconsole)
- [Creating a Pseudoconsole session](https://learn.microsoft.com/en-us/windows/console/creating-a-pseudoconsole-session)
