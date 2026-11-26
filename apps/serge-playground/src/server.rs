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

#[cfg(feature = "server")]
#[model]
#[derive(Clone)]
struct DemoUser {
    name: String,
}

#[cfg(feature = "server")]
impl SessionUser for DemoUser {
    fn principal(&self) -> String {
        self.name.clone()
    }
}

/// The playground signs visitors into one shared demo account. Browsers
/// receive an HttpOnly cookie and a CSRF token; desktop clients use Bearer.
#[server(POST "/demo/login")]
pub async fn demo_login(browser: bool) -> HyveResult<String> {
    let user = DemoUser {
        name: "demo".into(),
    };
    let client = if browser { Client::Web } else { Client::Bearer };
    let signed = orbic_auth::sign_in(&user, client).await?;
    signed
        .csrf_token
        .or(signed.access_token)
        .ok_or_else(|| HyveError::internal("missing demo session credential"))
}

#[server(POST "/demo/logout")]
pub async fn demo_logout() -> HyveResult<()> {
    orbic_auth::sign_out().await
}

/// The owner comes from the authenticated request. Only the ID and title
/// are returned to the UI.
#[cfg(feature = "server")]
#[table("projects")]
pub struct StoredProject {
    #[pk]
    pub id: u32,
    pub owner: String,
    pub title: String,
}

#[cfg(feature = "server")]
impl From<StoredProject> for Project {
    fn from(row: StoredProject) -> Self {
        Project {
            id: row.id,
            title: row.title,
        }
    }
}

#[server(POST "/projects")]
pub async fn create_project_rpc(
    #[server_ctx] db: Ctx<Db>,
    #[server_ctx] user: Ctx<DemoUser>,
    input: Valid<CreateProject>,
) -> HyveResult<Project> {
    sql!(
        StoredProject,
        "INSERT INTO projects (owner, title) VALUES ($1, $2) RETURNING id, owner, title",
        user.name,
        input.into_inner().title
    )
    .fetch_one(&*db)
    .await
    .map(Project::from)
}

#[server(GET "/projects")]
pub async fn list_projects(
    #[server_ctx] db: Ctx<Db>,
    #[server_ctx] user: Ctx<DemoUser>,
) -> HyveResult<Vec<Project>> {
    let rows = sql!(
        StoredProject,
        "SELECT id, owner, title FROM projects WHERE owner = $1 ORDER BY id",
        user.name
    )
    .fetch_all(&*db)
    .await?;
    Ok(rows.into_iter().map(Project::from).collect())
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
pub async fn backend(db: Db) -> HyveResult<OrbicApp> {
    db.migrate(&orbic::Migrations::new([(
        "0001_projects.sql".into(),
        include_str!("../../../migrations/0001_projects.sql").into(),
    )]))
    .await?;
    Ok(OrbicApp::new()
        .provide(Cache::from_env().await?)
        .provide(db)
        .mount(
            "/v1",
            component().handle(demo_login).handle(build_report).mount(
                "/",
                component()
                    .wrap(AuthGuard::<DemoUser>::require_user())
                    .handle(create_project_rpc)
                    .handle(list_projects)
                    .handle(demo_logout),
            ),
        ))
}

/// Attaches the production UI with the same session guard as the projects API.
#[cfg(feature = "web-host")]
pub fn with_web_bundle(app: OrbicApp, root: &std::path::Path) -> std::io::Result<OrbicApp> {
    use orbic::site::{Policies, SergeHost};
    let policy = Policies::new()
        .public("/")
        .guard("/projects", AuthGuard::<DemoUser>::require_user());
    Ok(app.provide(SergeHost::open(root, policy, None)?))
}
