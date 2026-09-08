//! Debug-only stdio adapter for isolated App integration tests.
//! It uses the real frame validator, dispatcher and store without user directories.
use crate::{
    app_dispatch,
    app_store::AppStore,
    protocol::{self, Request, Response},
};
use std::io;
use std::sync::{Arc, Mutex};

pub(crate) fn run() -> io::Result<()> {
    let root = std::env::args()
        .nth(2)
        .ok_or_else(|| io::Error::other("fixture root required"))?;
    let origin = std::env::args()
        .nth(3)
        .ok_or_else(|| io::Error::other("fixture origin required"))?;
    let root = std::path::PathBuf::from(root);
    std::fs::create_dir_all(&root)?;
    let store =
        AppStore::open_at(&root.join("natives.db"), root.join("apps")).map_err(io::Error::other)?;
    let manifests = if std::env::args().nth(4).as_deref() == Some("--chrome-profile") {
        root.join("profile/NativeMessagingHosts")
    } else {
        root.join("manifests")
    };
    store.set_manifest_dir(manifests);
    store.set_caller_origin(Some(&origin));
    store.recover_interrupted().map_err(io::Error::other)?;
    let mut input = io::stdin().lock();
    let writer = Arc::new(Mutex::new(io::stdout()));
    while let Some(bytes) = protocol::read_frame(&mut input) {
        let request: Request = serde_json::from_slice(&bytes)?;
        let result = protocol::validate_request(&request).and_then(|()| {
            app_dispatch::app_dispatch(&store, &request, store.caller_origin().as_deref())
        });
        let response = match result {
            Ok(result) => Response {
                id: &request.id,
                ok: true,
                result: Some(result),
                error: None,
            },
            Err(error) => Response {
                id: &request.id,
                ok: false,
                result: None,
                error: Some(protocol::safe_error(error)),
            },
        };
        protocol::respond(&writer, response)?;
    }
    Ok(())
}
