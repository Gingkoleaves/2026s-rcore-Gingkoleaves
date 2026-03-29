//!Implementation of [`TaskManager`]
use super::{TaskControlBlock,TaskPriority};
use crate::sync::UPSafeCell;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use lazy_static::*;
///A array of `TaskControlBlock` that is thread-safe
pub struct TaskManager {
    ready_queue: VecDeque<Arc<TaskControlBlock>>,
}

/// A simple FIFO scheduler.
impl TaskManager {
    ///Creat an empty TaskManager
    pub fn new() -> Self {
        Self {
            ready_queue: VecDeque::new(),
        }
    }
    /// Add process back to ready queue
    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        self.ready_queue.push_back(task);
    }
    /// Take a process out of the ready queue
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        if let Some(index)=self.find_candidate_task(){
            let task = self.ready_queue.remove(index).unwrap();
            task.inner_exclusive_access().inc_stride();
            Some(task)
        }else{
            None
        }
    }
    /// Find minial stride task, return idx
    pub fn find_candidate_task(&self)->Option<usize>{
        self.ready_queue.iter().enumerate().min_by(|(_, task_a), (_, task_b)| {
            let a_stride = task_a.inner_exclusive_access().get_stride();
            let b_stride = task_b.inner_exclusive_access().get_stride();

            // 使用之前定义的 Ord 实现（包含 wrapping_sub 逻辑）            
            TaskPriority::cmp(a_stride,b_stride)
        })
        .map(|(idx, _)| idx)
    }
}

lazy_static! {
    /// TASK_MANAGER instance through lazy_static!
    pub static ref TASK_MANAGER: UPSafeCell<TaskManager> =
        unsafe { UPSafeCell::new(TaskManager::new()) };
}

/// Add process to ready queue
pub fn add_task(task: Arc<TaskControlBlock>) {
    //trace!("kernel: TaskManager::add_task");
    TASK_MANAGER.exclusive_access().add(task);
}

/// Take a process out of the ready queue
pub fn fetch_task() -> Option<Arc<TaskControlBlock>> {
    //trace!("kernel: TaskManager::fetch_task");
    TASK_MANAGER.exclusive_access().fetch()
}
