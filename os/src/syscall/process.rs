//! Process management syscalls
use crate::{
    task::{exit_current_and_run_next, suspend_current_and_run_next, cur_syscall_count_get}, timer::get_time_us, syscall::NUM_SYSCALLS
};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(exit_code: i32) -> ! {
    trace!("[kernel] Application exited with code {}", exit_code);
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// get time with second and microsecond
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    unsafe {
        *ts = TimeVal {
            sec: us / 1_000_000,
            usec: us % 1_000_000,
        };
    }
    0
}

// return SYSCALL idx in COUNTER_SYSCALL
pub fn syscall_check(syscall_num:usize)->usize{
    match syscall_num{
        0..=NUM_SYSCALLS=>syscall_num,
        _=>panic!("Too big id for syscall!")
    }
}

// TODO: implement the syscall
pub fn sys_trace(_trace_request: usize, _id: usize, _data: usize) -> isize {
    trace!("kernel: sys_trace");
    match _trace_request{
        0=>{
            let ptr = _id as *const u8;
            let value:u8=unsafe{core::ptr::read_volatile(ptr)};
            value as isize
        },
        1=>{
            let ptr = _id as *mut u8;
            unsafe{ core::ptr::write_volatile(ptr, _data as u8);}
            0
        },
        2=>{
            let checked_syscall= syscall_check(_id);
            cur_syscall_count_get(checked_syscall) as isize 
        },
        _=> -1
    }
}
