//! Ordinary request admission is enforced by Core, independently of client limits.
use super::server::ServiceState;
use std::collections::HashMap;
use std::sync::Mutex;

pub(super) struct RequestLease<'a> {
    counts: &'a Mutex<HashMap<String, usize>>,
    owner: String,
}
pub(super) fn admit<'a>(state: &'a ServiceState, owner: &str) -> Result<RequestLease<'a>, ()> {
    let mut counts = state.request_counts.lock().map_err(|_| ())?;
    if counts.get(owner).copied().unwrap_or(0) >= 16 {
        return Err(());
    }
    *counts.entry(owner.into()).or_default() += 1;
    Ok(RequestLease {
        counts: &state.request_counts,
        owner: owner.into(),
    })
}
impl Drop for RequestLease<'_> {
    fn drop(&mut self) {
        if let Ok(mut counts) = self.counts.lock() {
            if let Some(count) = counts.get_mut(&self.owner) {
                *count -= 1;
                if *count == 0 {
                    counts.remove(&self.owner);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn requests_are_bounded_per_surface_and_release_their_slot() {
        let state = super::super::tests::make_test_state(0);
        let leases = (0..16)
            .map(|_| admit(&state, "first").unwrap())
            .collect::<Vec<_>>();
        assert!(admit(&state, "first").is_err());
        assert!(admit(&state, "second").is_ok());
        drop(leases);
        assert!(admit(&state, "first").is_ok());
        assert!(state.request_counts.lock().unwrap().is_empty());
    }
}
