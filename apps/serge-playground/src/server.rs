//! Models and `#[server]` functions shared by the UI and the backend. With
//! the `server` feature the bodies and the orbic app are compiled in;
//! without it (web and remote desktop clients) only call stubs remain.

use hyve::prelude::*;
#[cfg(feature = "server")]
use orbic::prelude::*;

#[model]
#[derive(Debug, Clone, PartialEq)]
pub struct CreateProject {
    #[validate(len = 1..=80)]
    pub title: String,
}

#[model]
#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    pub id: u32,
    pub title: String,
}

#[model]
#[derive(Debug, Clone, PartialEq)]
pub struct ReportRequest {
    #[validate(range = 0..=10000)]
    pub millis: u32,
}

/// The backend's projects (in memory: the playground runs without a
/// database).
#[cfg(feature = "server")]
#[derive(Debug, Default)]
pub struct ProjectStore(std::sync::Mutex<Vec<Project>>);

#[server(POST "/projects")]
pub async fn create_project_rpc(
    #[server_ctx] store: Ctx<ProjectStore>,
    input: Valid<CreateProject>,
) -> HyveResult<Project> {
    let mut list = store.0.lock().unwrap_or_else(|e| e.into_inner());
    let project = Project {
        id: list.len() as u32 + 1,
        title: input.into_inner().title,
    };
    list.push(project.clone());
    Ok(project)
}

#[server(GET "/projects")]
pub async fn list_projects(#[server_ctx] store: Ctx<ProjectStore>) -> HyveResult<Vec<Project>> {
    Ok(store.0.lock().unwrap_or_else(|e| e.into_inner()).clone())
}

/// A slow call, to watch cancellation: leaving the screen drops it.
#[server(POST "/reports")]
pub async fn build_report(input: Valid<ReportRequest>) -> HyveResult<String> {
    let millis = input.into_inner().millis;
    tokio::time::sleep(std::time::Duration::from_millis(millis.into())).await;
    Ok(format!("report ready after {millis} ms"))
}

/// The orbic app serving the functions under `/v1`.
#[cfg(feature = "server")]
pub fn backend() -> OrbicApp {
    OrbicApp::new()
        .provide(Cache::in_memory())
        .provide(ProjectStore::default())
        .mount(
            "/v1",
            component()
                .handle(create_project_rpc)
                .handle(list_projects)
                .handle(build_report),
        )
}
