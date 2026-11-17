use std::cell::UnsafeCell;
use std::io;
use std::mem::MaybeUninit;
use std::net::{TcpListener, TcpStream};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex, PoisonError};

extern "C" {
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
    fn setsid() -> i32;
    fn kill(pid: i32, sig: i32) -> i32;
    fn getrusage(who: i32, usage: *mut [i64; 18]) -> i32;
}

/// `c` keeps the listeners `fds` open across its exec.
pub fn keep_fds(c: &mut Command, fds: Vec<i32>) {
    // SAFETY: fcntl is async-signal-safe; between fork and exec it only clears close-on-exec on the listeners.
    unsafe {
        c.pre_exec(move || {
            if fds.iter().all(|&fd| fcntl(fd, 2, 0) != -1) {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        });
    }
}

/// The listeners at `fds` a predecessor passed down for this process alone, close-on-exec again.
pub fn adopt_listeners(fds: Vec<i32>) -> Vec<TcpListener> {
    // SAFETY: each fd is a listener the predecessor passed down for this process alone; nothing else here owns it.
    unsafe {
        fds.into_iter()
            .map(|fd| {
                fcntl(fd, 2, 1);
                TcpListener::from_raw_fd(fd)
            })
            .collect()
    }
}

/// `c` starts in a session (and process group) of its own.
pub fn new_session(c: &mut Command) {
    // SAFETY: setsid is async-signal-safe; between fork and exec it touches nothing else.
    unsafe {
        c.pre_exec(|| match setsid() {
            -1 => Err(io::Error::last_os_error()),
            _ => Ok(()),
        });
    }
}

/// SIGKILL to the process group `pid` leads.
pub fn kill_group(pid: u32) {
    let group = i32::try_from(pid).map_or(0, |p| -p);
    // SAFETY: kill(2) only sends SIGKILL to the child's own process group, which the runner made for it alone.
    unsafe { kill(group, 9) };
}

/// This process's `getrusage(2)` record (`RUSAGE_SELF`), as its 18 machine words.
pub fn rusage() -> Option<[i64; 18]> {
    let mut u = [0i64; 18];
    // SAFETY: an out buffer the size of `struct rusage` on 64-bit macOS and Linux.
    (unsafe { getrusage(0, &raw mut u) } == 0).then_some(u)
}

/// The APFS clone id of `p` when it shares its blocks with another file.
#[cfg(target_os = "macos")]
pub fn clone_id(p: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    #[repr(C)]
    struct AttrList([u16; 2], [u32; 5]);
    #[repr(C, packed)]
    struct Buf(u32, [u32; 5], u64, u64);
    extern "C" {
        fn getattrlist(
            path: *const std::ffi::c_char,
            list: *mut AttrList,
            buf: *mut Buf,
            size: usize,
            opts: u32,
        ) -> i32;
    }
    let path = std::ffi::CString::new(p.as_os_str().as_bytes()).ok()?;
    let mut list = AttrList([5, 0], [0x8000_0000, 0, 0, 0, 0x300]);
    let mut buf = Buf(0, [0; 5], 0, 0);
    // SAFETY: a NUL-terminated path, an attrlist and a buffer of the size passed, all live for the call.
    let rc = unsafe { getattrlist(path.as_ptr(), &raw mut list, &raw mut buf, size_of::<Buf>(), 0x21) };
    let (returned, id, flags) = (buf.1[4], buf.2, buf.3);
    (rc == 0 && returned & 0x300 == 0x300 && flags & 1 == 1).then_some(id)
}

/// The APFS clone id of `p`: none off macOS.
#[cfg(not(target_os = "macos"))]
pub fn clone_id(_: &Path) -> Option<u64> {
    None
}

/// Bytes written to `s` the peer has not taken yet.
#[cfg(target_os = "macos")]
pub fn unsent(s: &TcpStream) -> i32 {
    extern "C" {
        fn getsockopt(fd: i32, level: i32, name: i32, value: *mut i32, len: *mut u32) -> i32;
    }
    let (mut n, mut len) = (0i32, 4u32);
    // SAFETY: a live socket's fd and an int-sized out value with its length, as SO_NWRITE writes.
    let rc = unsafe { getsockopt(s.as_raw_fd(), 0xffff, 0x1024, &raw mut n, &raw mut len) };
    if rc == 0 {
        n
    } else {
        0
    }
}

/// Bytes written to `s` the peer has not taken yet.
#[cfg(not(target_os = "macos"))]
pub fn unsent(s: &TcpStream) -> i32 {
    extern "C" {
        fn ioctl(fd: i32, req: u64, ...) -> i32;
    }
    let mut n = 0i32;
    // SAFETY: a live socket's fd and an int out value, as SIOCOUTQ writes.
    let rc = unsafe { ioctl(s.as_raw_fd(), 0x5411, &raw mut n) };
    if rc == 0 {
        n
    } else {
        0
    }
}

/// Hand the allocator's free pages back to the OS.
#[cfg(target_os = "macos")]
pub fn release_freed() {
    extern "C" {
        fn malloc_zone_pressure_relief(zone: *mut std::ffi::c_void, goal: usize) -> usize;
    }
    // SAFETY: a null zone asks every malloc zone to return its free pages; goal 0 is as many as it can.
    unsafe { malloc_zone_pressure_relief(std::ptr::null_mut(), 0) };
}

/// Hand the allocator's free pages back to the OS.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
pub fn release_freed() {
    extern "C" {
        fn malloc_trim(pad: usize) -> i32;
    }
    // SAFETY: glibc's malloc_trim only returns free heap pages to the OS; pad 0 keeps no slack.
    unsafe { malloc_trim(0) };
}

/// Hand the allocator's free pages back to the OS: nothing to do on this platform.
#[cfg(not(any(target_os = "macos", all(target_os = "linux", target_env = "gnu"))))]
pub fn release_freed() {}

/// #944: an append-only buffer whose clones share one allocation and append to it in place; each sees only its own `len`.
pub struct Append<T> {
    buf: Arc<Cells<T>>,
    len: usize,
}

struct Cells<T> {
    len: Mutex<usize>,
    cells: Box<[UnsafeCell<MaybeUninit<T>>]>,
}

// SAFETY: a cell is written once, under `len`'s lock, before `len` passes it; only cells below `len` are ever read.
unsafe impl<T: Send + Sync> Sync for Cells<T> {}

impl<T> Drop for Cells<T> {
    fn drop(&mut self) {
        let n = *self.len.get_mut().unwrap_or_else(PoisonError::into_inner);
        for c in self.cells.iter_mut().take(n) {
            // SAFETY: the first `n` cells were written (`push`) and are dropped only here.
            unsafe { c.get_mut().assume_init_drop() };
        }
    }
}

impl<T> Default for Append<T> {
    fn default() -> Self {
        Append::with_capacity(0)
    }
}

impl<T> Clone for Append<T> {
    fn clone(&self) -> Self {
        Append {
            buf: self.buf.clone(),
            len: self.len,
        }
    }
}

impl<T> std::ops::Deref for Append<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        // SAFETY: the first `len` cells are written and never written again; a cell is laid out as its `T`.
        unsafe { std::slice::from_raw_parts(self.buf.cells.as_ptr().cast(), self.len) }
    }
}

impl<T> Append<T> {
    fn with_capacity(cap: usize) -> Self {
        let cells = (0..cap).map(|_| UnsafeCell::new(MaybeUninit::uninit())).collect();
        let buf = Arc::new(Cells {
            len: Mutex::new(0),
            cells,
        });
        Append { buf, len: 0 }
    }

    /// The first `n` items (at most all), sharing this buffer.
    #[must_use]
    pub fn prefix(&self, n: usize) -> Self {
        Append {
            buf: self.buf.clone(),
            len: n.min(self.len),
        }
    }
}

impl<T: Clone> Append<T> {
    /// Append `e`, in place when no later version has appended past this one, else in a fresh copy.
    pub fn push(&mut self, e: T) {
        loop {
            let mut at = self.buf.len.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(c) = self.buf.cells.get(self.len).filter(|_| *at == self.len) {
                // SAFETY: cell `len` is past what every handle sees and `at`'s lock is held: nothing else touches it.
                unsafe { (*c.get()).write(e) };
                (*at, self.len) = (self.len + 1, self.len + 1);
                return;
            }
            drop(at);
            self.grow(1);
        }
    }

    /// Room for `n` more items without a copy.
    pub fn reserve(&mut self, n: usize) {
        if self.len + n > self.buf.cells.len() {
            self.grow(n);
        }
    }

    fn grow(&mut self, n: usize) {
        let mut next = Append::with_capacity((self.len + n).next_power_of_two().max(64));
        self.iter().for_each(|e| next.push(e.clone()));
        *self = next;
    }
}
