//! Private task receipts stay with the authenticated Surface that created them.
use app_runtime_core::{http::HttpRequest, module::ModuleHttpResponse};
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
pub(crate) enum Kind {
    Information,
    Opportunity,
    Notification,
}

impl Kind {
    pub(crate) fn route(method: &str, path: &str) -> Option<(Self, bool)> {
        match (method, path) {
            ("POST", "/api/information/refresh" | "/api/information/content") => {
                Some((Self::Information, true))
            }
            ("GET", "/api/information/status") | ("POST", "/api/information/cancel") => {
                Some((Self::Information, false))
            }
            ("POST", "/api/opportunities/refresh") => Some((Self::Opportunity, true)),
            ("GET", "/api/opportunities/status") | ("POST", "/api/opportunities/cancel") => {
                Some((Self::Opportunity, false))
            }
            ("POST", "/api/notify/send" | "/api/notify/channels/test") => {
                Some((Self::Notification, true))
            }
            ("GET", "/api/notify/status") | ("POST", "/api/notify/cancel") => {
                Some((Self::Notification, false))
            }
            _ => None,
        }
    }
}

struct Owner {
    surface_id: String,
    generation: String,
    released: bool,
    request_id: String,
    task_id: u64,
}
#[derive(Default)]
pub(crate) struct TaskOwners(Mutex<HashMap<Kind, Owner>>);

impl TaskOwners {
    pub(crate) fn run(
        &self,
        identity: (&str, &str),
        kind: Kind,
        start: bool,
        finished: bool,
        request: &HttpRequest,
        invoke: impl FnOnce() -> Result<ModuleHttpResponse, (u16, String)>,
    ) -> Result<ModuleHttpResponse, (u16, String)> {
        let (surface_id, generation) = identity;
        let mut owners = self
            .0
            .lock()
            .map_err(|_| (503, "APP_TASK_OWNER_UNAVAILABLE".into()))?;
        let input: serde_json::Value = serde_json::from_slice(&request.body).unwrap_or_default();
        let request_id = input["requestId"].as_str().unwrap_or("");
        if let Some(owner) = owners.get(&kind) {
            if owner.surface_id != surface_id || owner.generation != generation || owner.released {
                if !start || owner.request_id == request_id {
                    return Err((404, "APP_TASK_NOT_FOUND".into()));
                }
                if !finished {
                    return Err((409, "APP_TASK_IN_PROGRESS".into()));
                }
            }
        } else if !start {
            return Err((404, "APP_TASK_NOT_FOUND".into()));
        }
        let response = invoke()?;
        if start {
            let result: serde_json::Value = serde_json::from_slice(&response.body)
                .map_err(|_| (500, "APP_RESPONSE_INVALID".into()))?;
            if let Some(task_id) = result["taskId"].as_u64() {
                owners.insert(
                    kind,
                    Owner {
                        surface_id: surface_id.into(),
                        generation: generation.into(),
                        released: false,
                        request_id: request_id.into(),
                        task_id,
                    },
                );
            }
        }
        Ok(response)
    }

    pub(crate) fn release(&self, surface_id: &str) -> Vec<(Kind, u64)> {
        let Ok(mut owners) = self.0.lock() else {
            return Vec::new();
        };
        let kinds = owners
            .iter()
            .filter(|(_, owner)| owner.surface_id == surface_id && !owner.released)
            .map(|(kind, owner)| (*kind, owner.task_id))
            .collect::<Vec<_>>();
        for (kind, _) in &kinds {
            if let Some(owner) = owners.get_mut(kind) {
                owner.released = true;
            }
        }
        kinds
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(id: &str) -> HttpRequest {
        let body = serde_json::json!({"requestId": id}).to_string();
        let bytes = format!("POST /api/information/refresh HTTP/1.1\r\nHost: 127.0.0.1:12345\r\nOrigin: null\r\nContent-Length: {}\r\n\r\n{body}", body.len());
        HttpRequest::read(&mut bytes.as_bytes()).unwrap()
    }

    #[test]
    fn another_surface_cannot_read_cancel_or_replay_private_tasks() {
        let owners = TaskOwners::default();
        let a = request("request-a");
        owners
            .run(
                ("surface-a", "generation-1"),
                Kind::Information,
                true,
                true,
                &a,
                || Ok(ModuleHttpResponse::ok_json("{\"taskId\":7}")),
            )
            .unwrap();
        assert_eq!(
            owners
                .run(
                    ("surface-b", "generation-1"),
                    Kind::Information,
                    false,
                    false,
                    &a,
                    || unreachable!()
                )
                .unwrap_err()
                .0,
            404
        );
        assert_eq!(
            owners
                .run(
                    ("surface-b", "generation-1"),
                    Kind::Information,
                    true,
                    true,
                    &a,
                    || unreachable!()
                )
                .unwrap_err()
                .0,
            404
        );
        let b = request("request-b");
        assert_eq!(
            owners
                .run(
                    ("surface-b", "generation-1"),
                    Kind::Information,
                    true,
                    false,
                    &b,
                    || unreachable!()
                )
                .unwrap_err()
                .0,
            409
        );
        let cancelled = owners.release("surface-a");
        assert_eq!(cancelled.len(), 1);
        assert_eq!(cancelled[0].1, 7);
        assert_eq!(
            owners
                .run(
                    ("surface-a", "generation-2"),
                    Kind::Information,
                    false,
                    true,
                    &a,
                    || unreachable!()
                )
                .unwrap_err()
                .0,
            404
        );
        assert_eq!(
            owners
                .run(
                    ("surface-b", "generation-1"),
                    Kind::Information,
                    true,
                    true,
                    &a,
                    || unreachable!()
                )
                .unwrap_err()
                .0,
            404
        );
        owners
            .run(
                ("surface-b", "generation-1"),
                Kind::Information,
                true,
                true,
                &b,
                || Ok(ModuleHttpResponse::ok_json("{\"taskId\":8}")),
            )
            .unwrap();
        assert!(owners.release("surface-a").is_empty());
        assert_eq!(owners.release("surface-b")[0].1, 8);
    }
}
