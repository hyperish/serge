use std::sync::Arc;

use hyve::Scope;
use hyve::api::{Transport, provide_transport};
use orbic::{Db, OrbicApp};
use serge_playground::{PlaygroundSession, backend};

/// Each test has its own database, migrated by the same app as the server binary.
pub fn database_backend() -> (Db, OrbicApp) {
    serge::runtime().block_on(async {
        let db = Db::empty_in_memory().await;
        let app = backend(db.clone()).await.unwrap();
        (db, app)
    })
}

pub fn in_process() -> PlaygroundSession {
    PlaygroundSession::in_process(database_backend().1.build().unwrap())
}

pub fn scope_with(transport: Arc<dyn Transport>) -> Scope {
    let scope = Scope::root();
    provide_transport(&scope, transport);
    scope
}

pub fn run<F: std::future::IntoFuture>(scope: &Scope, call: impl FnOnce() -> F) -> F::Output {
    let call = scope.enter(|| call().into_future());
    serge::runtime().block_on(scope.scoped(call))
}

pub fn signed_in(session: PlaygroundSession) -> Scope {
    let scope = scope_with(Arc::new(session.clone()));
    run(&scope, || session.sign_in()).unwrap();
    scope
}
