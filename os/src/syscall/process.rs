//! Process management syscalls
use crate::mm::{MapPermission, VirtAddr ,PageTable, copy_to_user};
use crate::task::{handle_cur_page_fault, change_program_brk, cur_syscall_count_get, current_mmap, current_munmap, current_user_token, exit_current_and_run_next, suspend_current_and_run_next
                };
use crate::config::{CLOCK_FREQ, MAX_SYSCALL_NUM, PAGE_SIZE};
use crate::timer::{get_time};

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
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
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

/// TODO: Finish sys_trace to pass testcases
/// HINT: You might reimplement it with virtual memory management.
pub fn sys_trace(_trace_request: usize, _id: usize, _data: usize) -> isize {
    trace!("kernel: sys_trace");
    let token=current_user_token();
    match _trace_request{
        0=>{
            let page_table = PageTable::from_token(token);
            let va = VirtAddr::from(_id);
            let vpn = va.floor();

            // 如果vaild=1或vaild=0但是不是lazy-allocate，则不处理
            handle_cur_page_fault(va);

            match page_table.translate(vpn) {
                Some(pte) if pte.is_valid() && pte.is_user_allowed() && pte.readable()=> {
                    pte.ppn().get_bytes_array()[va.page_offset()] as isize
                }
                _ => -1,
            }
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

// YOUR JOB: Implement mmap.
pub fn sys_mmap(_start: usize, _len: usize, _prot: usize) -> isize {
    trace!("kernel: sys_mmap NOT IMPLEMENTED YET!");

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

// YOUR JOB: Implement munmap.
pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    trace!("kernel: sys_munmap NOT IMPLEMENTED YET!");

    // not aligned
    if _start & (PAGE_SIZE-1)!=0{
        return -1;
    }

    // not aligned
    if _len & (PAGE_SIZE - 1) != 0 {
        return -1;
    }

    let start_va = VirtAddr::from(_start);
    let end_va =VirtAddr::from(_start+_len);

    current_munmap(start_va, end_va)
}
/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}


