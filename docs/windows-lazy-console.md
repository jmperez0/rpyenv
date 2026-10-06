# Avoiding unnecessary console windows on Windows

**Status:** implemented in M5a: EAGER, LAZY, the manifest entry, holding a window,
and waiting for the program when a window closes. Detecting input without prior output
stays deferred.
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
   ├─ not the console shim, or 24H2
   │  APIs unavailable? ───────────────── yes → MIRROR
   ├─ the parent has a console, or
   │  the caller gave any standard handle, or
   │  RPYENV_CONSOLE=eager? ────────────── yes → EAGER
   └─ otherwise ─────────────────────────────── → LAZY
```

**The parent-console rule.** With the manifest entry, a shim the caller started with
`DETACHED_PROCESS` and one started with no flags by a caller without a console (Explorer)
look the same at startup. Both have no console, and the process parameters'
`ConsoleHandle` is 0 for both. That was probed on build 26200, by logging it in a child
started each way. Only `AllocConsoleWithOptions(DEFAULT)` tells them apart. So a shim
whose parent has a console takes EAGER:
- a `DETACHED_PROCESS` caller gets no console, and the child then starts with
  `DETACHED_PROCESS`, exactly as MIRROR;
- a `CREATE_NEW_CONSOLE` caller (cmd's `start`) gets its new window at once, as with
  `python.exe`.

The shim checks for the parent's console with `AttachConsole(ATTACH_PARENT_PROCESS)` and
leaves it at once, keeping its standard handles as they were. A caller that passes
`CREATE_NO_WINDOW` still gives the shim a windowless console at startup, so the shim takes
INHERIT.

**Given handles.** A caller that gave the shim any standard handle (stdin from a file or a
pipe, output to NUL) gets EAGER, so the child keeps those handles. LAZY would replace them
with the pseudo-console's. Explorer gives none.

`pyenv exec` and the GUI shim don't carry the manifest entry and keep the first three modes.

| Mode | Child is started with | Window appears |
|---|---|---|
| INHERIT | The shim's console and std handles, inherited | Whatever the caller arranged. This is the normal case in a terminal. |
| NO-WINDOW | `CREATE_NO_WINDOW`; the caller's redirected handles; stdin set to `NUL` if the caller didn't provide it | Never |
| MIRROR | `DETACHED_PROCESS` | Never. Same as `python.exe` started with that flag. |
| EAGER | `AllocConsoleWithOptions(DEFAULT)` at startup, then INHERIT, or `DETACHED_PROCESS` when that gives no console | At startup, as with `python.exe`; after a failure it may stay open (step 6) |
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
   `CreatePseudoConsole` with an initial size of 120×30. That is Windows 11's default
   console size, for both conhost and Windows Terminal, and it lasts only until a window
   appears.

   Start the child with `PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE`, inside the shim's Job Object
   (the same one that kills the child if the shim dies):
   - **`CreateProcessW` directly**, with `"<program>"` followed by the shim's own
     command-line tail, unchanged. std's attribute-list API (`raw_attribute`) is still
     unstable.
   - **`STARTF_USESTDHANDLES` with null handles.** Without it, the child inherited the
     shim's own standard handles, and its output bypassed the pseudo-console (probed on
     build 26200).
   - **A batch target takes EAGER instead,** because only std's batch-file escaping makes
     starting one safe.
2. **Always drain the output.** A dedicated thread reads the pseudo-console's
   output pipe continuously. If the pipe fills up, the child blocks. Until the
   window is shown, the bytes are buffered in memory.
3. **Watch for the trigger.** Scan the output as a VT stream:
   - skip escape sequences (CSI `ESC [ … final`, OSC `ESC ] … BEL|ST`,
     DCS/SOS/PM/APC `… ST`, two-byte `ESC x`),
   - skip C0 control characters and whitespace.

   The first character left over is the trigger. ConPTY emits its own setup
   sequences, so "first byte" would fire immediately. On build 26200 it starts with
   `ESC[?9001h ESC[?1004h` (win32-input-mode, focus events). Then come
   `ESC[?25l ESC[2J ESC[m ESC[H`, the program's text, `ESC]0;<program path> BEL` and
   `ESC[?25h`. A program that prints nothing produces only sequences. These captures
   are the scanner's test fixtures (`rpyenv_core::vtscan`).
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
   - write the buffered output, then keep relaying, holding back a UTF-8 character that
     a read cut in two until it is whole
   - the window title comes from ConPTY's own `OSC 0` (the program's path), as when
     `python.exe` runs directly

   If `AllocConsoleWithOptions` reports that the caller wanted no console (a
   `DETACHED_PROCESS` caller whose parent has none), the shim keeps draining the output
   and drops it. A program that then reads input waits for it, whether or not it printed
   first, where `python.exe` started that way would get end-of-file. The workaround is
   the same as for the deferred case below: `RPYENV_CONSOLE=eager`.
5. **Relay input.** An input thread waits on the console input and a stop event, then
   calls `ReadConsoleInputW`. Processed, line and echo input are off, so Ctrl+C arrives
   as a key.
   - **While ConPTY asks for win32-input-mode** (`?9001h`, at startup), each
     `KEY_EVENT_RECORD` goes as `ESC[Vk;Sc;Uc;Kd;Cs;Rc_`. This is lossless: key-ups,
     modifiers, and Ctrl+C, which ConPTY turns into `CTRL_C_EVENT` for the program.
   - **Otherwise** the key-downs' characters go as text, with
     `ENABLE_VIRTUAL_TERMINAL_INPUT`.
   - `WINDOW_BUFFER_SIZE_EVENT` becomes `ResizePseudoConsole`.

   Limitation: Ctrl+Break in the window reaches the shim, which ignores it, not the program.
   A program that inherited "ignore Ctrl+C" from its caller (a parent's
   `SetConsoleCtrlHandler(NULL, TRUE)`) ignores it here too, as it would without the shim.
6. **Shut down.** When the child exits, keep the pseudo-console while processes it
   left running still use it, as `python.exe`'s console stays for them. ConPTY never
   closes on its own, even with no process left (probed: 10 s each, three cases).
   - A process snapshot first looks for processes that may still use the pseudo-console:
     the child's live descendants, and processes created after it whose parent has exited.
     A launcher, such as a venv's `python.exe` or pip's `black.exe`, exits together with
     the real program it started.
   - If there are any, the shim starts a watcher on the pseudo-console: its own binary,
     flagged by an internal variable that names the shim as its parent. The watcher
     exits once it is the only process there, so processes on another console, or with
     none, don't keep the shim.
   - A caller that waits for the shim (Task Scheduler, `WshShell.Run …, True`) gets the
     exit code only after those processes end, unlike with `python.exe`.

   Then keep reading the output pipe until it closes, and call `ClosePseudoConsole` from a
   thread other than the reader.
   Before 24H2, `ClosePseudoConsole` waits until the output is drained. The
   shim then returns the child's exit code.
   - **User closes the window, logs off or shuts down:** the shim gets
     `CTRL_CLOSE_EVENT` (or LOGOFF or SHUTDOWN). It calls `ClosePseudoConsole`, which
     sends `CTRL_CLOSE_EVENT` to the child, then waits for the child before returning,
     until Windows' own timeout ends both. Returning at once would let Windows end the
     shim and the job end the child in the middle of its cleanup. The handler does the
     same in every mode. On a shared console the child is usually done first anyway: the
     console host closes the most recently attached process first.
   - **Failure after the shim opened a new window** (`ALLOC_CONSOLE_RESULT_NEW_CONSOLE`,
     in EAGER or LAZY): print the exit code and keep the window, so a traceback from a
     double-clicked script stays readable. `RPYENV_CONSOLE_HOLD` sets how long
     (Configuration). There is no hold:
     - for a program ended by Ctrl+C (`0xC000013A`), or that exits 130, as programs that
       catch Ctrl+C commonly do;
     - when nobody can see the window, because nobody could press the key: the window
       station isn't visible (a scheduled task that runs whether or not a user is logged
       on, a service), or the caller hid the window (`WshShell.Run …, 0, True`);
     - when a console parent asked for the window (`start /wait` in a script), which
       needs the errorlevel, as with `python.exe`. A modifier or
     lock key pressed alone (Ctrl, as the start of Ctrl+C to copy the text) doesn't close
     the window. In INHERIT mode this never happens, because the terminal belongs to the
     caller.
   - **The shim's own error, with nowhere to print it** (a version that isn't installed,
     say): the console shim asks for the caller's console the same way, prints the
     message, and holds the window. The same goes for a program EAGER can't start after
     opening a window for it.

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

- `lazy`: as described above. This is the default (decided 2026-10-06; cost in open
  question 3).
- `eager`: behave like `python.exe` and create the console at startup

`RPYENV_CONSOLE_HOLD` selects what happens to a window the shim opened after the program
fails:

- unset, or anything other than a whole number: wait for a key
- `0`: close at once, as `python.exe` does
- a positive whole number N: close after N seconds, or sooner on a key

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

1. **Explorer launches with `ALLOC_CONSOLE_MODE_DEFAULT`.** A launch with no flags from a
   process with no console got a new window (result 1) in the probe and in the e2e tests,
   which start the shim that way. Still to confirm on a real Explorer double-click
   (manual checklist).
2. **Initial pseudo-console size.** Resolved: a fixed 120×30 (LAZY step 1).
3. **Cost of ConPTY startup.** Resolved: `lazy` is the default. Measured on build 26200
   as the median of 30 launches of a trivial program, from start to exit (probe
   `conpty time`):
   - on a pseudo-console: 715 ms;
   - with `CREATE_NO_WINDOW` (a fresh windowless conhost): 649 ms;
   - on an inherited console: 86 ms.

   A launch with no console gets a fresh console either way, so LAZY's extra cost on that
   host is about 66 ms (715 − 649).
4. **Hold on error.** Resolved: on by default, configurable with `RPYENV_CONSOLE_HOLD`.

## References

- [Console Allocation Policy](https://learn.microsoft.com/en-us/windows/console/console-allocation-policy)
- [AllocConsoleWithOptions](https://learn.microsoft.com/en-us/windows/console/allocconsolewithoptions)
  and [ALLOC_CONSOLE_OPTIONS](https://learn.microsoft.com/en-us/windows/console/alloc-console-options)
- [Spec #7335: Console Allocation Policy](https://github.com/microsoft/terminal/blob/main/doc/specs/%237335%20-%20Console%20Allocation%20Policy.md)
- [ClosePseudoConsole](https://learn.microsoft.com/en-us/windows/console/closepseudoconsole)
- [Creating a Pseudoconsole session](https://learn.microsoft.com/en-us/windows/console/creating-a-pseudoconsole-session)
