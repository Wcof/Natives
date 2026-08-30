use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

pub(crate) type ActiveJobs = Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>;

pub(crate) fn reap_finished_jobs(jobs: &mut Vec<thread::JoinHandle<()>>) {
    let mut index = 0;
    while index < jobs.len() {
        if jobs[index].is_finished() {
            let job = jobs.swap_remove(index);
            let _ = job.join();
        } else {
            index += 1;
        }
    }
}

pub(crate) fn shutdown_jobs(active: &ActiveJobs, jobs: &mut Vec<thread::JoinHandle<()>>) {
    if let Ok(active) = active.lock() {
        for token in active.values() {
            token.store(true, Ordering::Relaxed);
        }
    }
    for job in jobs.drain(..) {
        let _ = job.join();
    }
    if let Ok(mut active) = active.lock() {
        active.clear();
    }
}
