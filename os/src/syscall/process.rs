//! Process management syscalls
use alloc::sync::Arc;

use crate::{
    loader::get_app_data_by_name,
    mm::{translated_refmut, translated_str, copy_to_user, MapPermission, VirtAddr },
    task::{
        add_task, current_task, current_user_token, exit_current_and_run_next,
        suspend_current_and_run_next, current_mmap, current_munmap, handle_cur_page_fault,
        cur_syscall_count_get, current_task_set_prio, MIN_PRIORITY
    },
    config::{MAX_SYSCALL_NUM,CLOCK_FREQ,PAGE_SIZE},
    timer::{get_time}
};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

// return SYSCALL idx in COUNTER_SYSCALL
pub fn check_syscall(syscall_num:usize)->usize{
    match syscall_num{
        0..=MAX_SYSCALL_NUM=>syscall_num,
        _=>panic!("Too big id for syscall!")
    }
}

/// task exits and submit an exit code
pub fn sys_exit(exit_code: i32) -> ! {
    trace!("kernel:pid[{}] sys_exit", current_task().unwrap().pid.0);
    exit_current_and_run_next(exit_code);
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel:pid[{}] sys_yield", current_task().unwrap().pid.0);
    suspend_current_and_run_next();
    0
}

pub fn sys_getpid() -> isize {
    trace!("kernel: sys_getpid pid:{}", current_task().unwrap().pid.0);
    current_task().unwrap().pid.0 as isize
}

pub fn sys_fork() -> isize {
    trace!("kernel:pid[{}] sys_fork", current_task().unwrap().pid.0);
    let current_task = current_task().unwrap();
    let new_task = current_task.fork();
    let new_pid = new_task.pid.0;
    // modify trap context of new_task, because it returns immediately after switching
    let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
    // we do not have to move to next instruction since we have done it before
    // for child process, fork returns 0
    trap_cx.x[10] = 0;
    // add new task to scheduler
    add_task(new_task);
    new_pid as isize
}

pub fn sys_exec(path: *const u8) -> isize {
    trace!("kernel:pid[{}] sys_exec", current_task().unwrap().pid.0);
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(data) = get_app_data_by_name(path.as_str()) {
        let task = current_task().unwrap();
        task.exec(data);
        0
    } else {
        -1
    }
}

/// If there is not a child process whose pid is same as given, return -1.
/// Else if there is a child process but it is still running, return -2.
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    trace!("kernel::pid[{}] sys_waitpid [{}]", current_task().unwrap().pid.0, pid);
    let task = current_task().unwrap();
    // find a child process

    // ---- access current PCB exclusively
    let mut inner = task.inner_exclusive_access();
    if !inner
        .children
        .iter()
        .any(|p| pid == -1 || pid as usize == p.getpid())
    {
        return -1;
        // ---- release current PCB
    }
    let pair = inner.children.iter().enumerate().find(|(_, p)| {
        // ++++ temporarily access child PCB exclusively
        p.inner_exclusive_access().is_zombie() && (pid == -1 || pid as usize == p.getpid())
        // ++++ release child PCB
    });
    if let Some((idx, _)) = pair {
        let child = inner.children.remove(idx);
        // confirm that child will be deallocated after being removed from children list
        assert_eq!(Arc::strong_count(&child), 1);
        let found_pid = child.getpid();
        // ++++ temporarily access child PCB exclusively
        let exit_code = child.inner_exclusive_access().exit_code;
        // ++++ release child PCB
        *translated_refmut(inner.memory_set.token(), exit_code_ptr) = exit_code;
        found_pid as isize
    } else {
        -2
    }
    // ---- release current PCB automatically
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let time =TimeVal{
                sec:get_time()/CLOCK_FREQ,
                usec:(get_time() % CLOCK_FREQ) * 1000000 / CLOCK_FREQ
            };

    let src=unsafe{
        core::slice::from_raw_parts(
            &time as *const _ as *const u8,
            core::mem::size_of::<TimeVal>()
        ) 
    };

    copy_to_user(current_user_token(), src, _ts as usize)
}

/// YOUR JOB: Implement mmap.
pub fn sys_mmap(_start: usize, _len: usize, _prot: usize) -> isize {
    trace!(
        "kernel:pid[{}] sys_mmap NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    // not aligned
    if _start & (PAGE_SIZE-1)!=0{
        return -1;
    }
    // illegal prot or meaningless prot
    if _prot&!0x7 !=0 || _prot&0x7 ==0{
        return -1
    } 

    let start_va = VirtAddr::from(_start);
    let end_va =VirtAddr::from(_start+_len);
    let mut map_prem=MapPermission::from_bits_truncate((_prot<< 1)as u8);
    map_prem |=MapPermission::U;
    
    current_mmap(start_va, end_va, map_prem)
}

/// YOUR JOB: Implement munmap.
pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    trace!(
        "kernel:pid[{}] sys_munmap NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    // not aligned
    if _start & (PAGE_SIZE-1)!=0{
        return -1;
    }

    // not aligned
    if _len & (PAGE_SIZE - 1) != 0 {
        return -1;
    }

    let start_va = VirtAddr::from(_start);
    let end_va = VirtAddr::from(_start+_len);

    current_munmap(start_va, end_va)
}

/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel:pid[{}] sys_sbrk", current_task().unwrap().pid.0);
    if let Some(old_brk) = current_task().unwrap().change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}

/// YOUR JOB: Implement spawn.
/// HINT: fork + exec =/= spawn
pub fn sys_spawn(_path: *const u8) -> isize {
    trace!(
        "kernel:pid[{}] sys_spawn NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    let token = current_user_token();
    let path = translated_str(token, _path);
    if let Some(data) = get_app_data_by_name(path.as_str()) {
        let task = current_task().unwrap();
        let new_task=task.spawn(data);// 获取子进程的 PID，作为系统调用的返回值
        let new_pid = new_task.pid.0;
        // add new task to scheduler
        add_task(new_task);
        new_pid as isize
    } else {
        -1
    }
}

// YOUR JOB: Set task priority.
pub fn sys_set_priority(_prio: isize) -> isize {
    trace!(
        "kernel:pid[{}] sys_set_priority NOT IMPLEMENTED",
        current_task().unwrap().pid.0
    );
    if _prio<MIN_PRIORITY as isize{
        return -1 
    }

    current_task_set_prio(_prio as usize);
    _prio
}

#[allow(unused)]
pub fn sys_trace(_trace_request: usize, _id: usize, _data: usize) -> isize {
    trace!("kernel: sys_trace");
    let token=current_user_token();
    match _trace_request{
        0=>{
            // 如果vaild=1或vaild=0但是不是lazy-allocate，则不处理
            handle_cur_page_fault(_id.into());
            // translated_refmut遇到非法va直接unwrap->panic
            *(translated_refmut(token,_id as *mut u8)) as isize
        },
        1=>{
            let src: [u8; 1] = (_data as u8).to_le_bytes();
            // 如果vaild=1或vaild=0但是不是lazy-allocate，则不处理
            handle_cur_page_fault(_id.into());
            copy_to_user(token, &src, _id)
        },
        2=>{
            let checked_syscall= check_syscall(_id);
            cur_syscall_count_get(checked_syscall) as isize 
        },
        _=> -1
    }
}