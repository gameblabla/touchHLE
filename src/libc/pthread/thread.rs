//! Threads.

use crate::abi::GuestFunction;
use crate::dyld::{export_c_func, FunctionExports};
use crate::mem::{ConstPtr, MutPtr, MutVoidPtr, Ptr, SafeRead};
use crate::{Environment, ThreadID};
use std::collections::HashMap;

#[derive(Default)]
pub struct State {
    threads: HashMap<pthread_t, ThreadHostObject>,
}
impl State {
    fn get(env: &mut Environment) -> &mut Self {
        &mut env.libc_state.pthread.thread
    }
}

/// Apple's implementation is a 4-byte magic number followed by an 36-byte
/// opaque region. We only have to match the size theirs has.
#[derive(Copy, Clone, Debug)]
#[repr(C, packed)]
struct pthread_attr_t {
    /// Magic number (must be [MAGIC_ATTR])
    magic: u32,
    detachstate: i32,
    _unused: [u32; 8],
}
unsafe impl SafeRead for pthread_attr_t {}

const DEFAULT_ATTR: pthread_attr_t = pthread_attr_t {
    magic: MAGIC_ATTR,
    detachstate: PTHREAD_CREATE_JOINABLE,
    _unused: [0; 8],
};

/// Apple's implementation is a 4-byte magic number followed by a massive
/// (>4KiB) opaque region. We will store the actual data on the host instead.
#[repr(C, packed)]
struct OpaqueThread {
    /// Magic number (must be [MAGIC_THREAD])
    magic: u32,
}
unsafe impl SafeRead for OpaqueThread {}

type pthread_t = MutPtr<OpaqueThread>;

struct ThreadHostObject {
    _thread_id: ThreadID,
    _attr: pthread_attr_t,
}

/// Arbitrarily-chosen magic number for `pthread_attr_t` (not Apple's).
const MAGIC_ATTR: u32 = u32::from_be_bytes(*b"ThAt");
/// Arbitrarily-chosen magic number for `pthread_t` (not Apple's).
const MAGIC_THREAD: u32 = u32::from_be_bytes(*b"THRD");

/// Custom typedef for readability (the C API just uses `int`)
type DetachState = i32;
const PTHREAD_CREATE_JOINABLE: DetachState = 1;
const PTHREAD_CREATE_DETACHED: DetachState = 2;

fn pthread_attr_init(env: &mut Environment, attr: MutPtr<pthread_attr_t>) -> i32 {
    env.mem.write(attr, DEFAULT_ATTR);
    0 // success
}
fn pthread_attr_setdetachstate(
    env: &mut Environment,
    attr: MutPtr<pthread_attr_t>,
    detachstate: DetachState,
) -> i32 {
    check_magic!(env, attr, MAGIC_ATTR);
    assert!(detachstate == PTHREAD_CREATE_JOINABLE || detachstate == PTHREAD_CREATE_DETACHED); // should be EINVAL
    let mut attr_copy = env.mem.read(attr);
    attr_copy.detachstate = detachstate;
    env.mem.write(attr, attr_copy);
    0 // success
}
fn pthread_attr_destroy(env: &mut Environment, attr: MutPtr<pthread_attr_t>) -> i32 {
    check_magic!(env, attr, MAGIC_ATTR);
    env.mem.write(
        attr,
        pthread_attr_t {
            magic: 0,
            detachstate: 0,
            _unused: Default::default(),
        },
    );
    0 // success
}

fn pthread_create(
    env: &mut Environment,
    thread: MutPtr<pthread_t>,
    attr: ConstPtr<pthread_attr_t>,
    start_routine: GuestFunction, // (*void)(void *)
    user_data: MutVoidPtr,
) -> i32 {
    let attr = if !attr.is_null() {
        check_magic!(env, attr, MAGIC_ATTR);
        env.mem.read(attr)
    } else {
        DEFAULT_ATTR
    };

    let thread_id = env.new_thread(start_routine, user_data);

    let opaque = env.mem.alloc_and_write(OpaqueThread {
        magic: MAGIC_THREAD,
    });
    env.mem.write(thread, opaque);

    assert!(!State::get(env).threads.contains_key(&opaque));
    State::get(env).threads.insert(
        opaque,
        ThreadHostObject {
            _thread_id: thread_id,
            _attr: attr,
        },
    );

    log_dbg!("pthread_create({:?}, {:?}, {:?}, {:?}) => 0 (success), created new pthread_t {:?} (thread ID: {})", thread, attr, start_routine, user_data, opaque, thread_id);

    0 // success
}

fn pthread_self(_env: &mut Environment) -> pthread_t {
    // FIXME: Implement this for real. Super Monkey Ball conveniently checks
    // if this returns zero and skips some code for querying thread properties
    // if so, even though zero isn't a meaningful value...
    log!("Warning: TODO: pthread_self() (returning 0)");
    Ptr::null()
}

pub const FUNCTIONS: FunctionExports = &[
    export_c_func!(pthread_attr_init(_)),
    export_c_func!(pthread_attr_setdetachstate(_, _)),
    export_c_func!(pthread_attr_destroy(_)),
    export_c_func!(pthread_create(_, _, _, _)),
    export_c_func!(pthread_self()),
];