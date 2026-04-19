use super::*;

pub(crate) fn render_compose_project(
    path: &Path,
    list_services: bool,
) -> Result<RenderedComposeProject, Error> {
    let project = ComposeProjectPlan::load(path)?;
    let _catalog = project.clone().into_service_catalog()?;
    let warnings = if list_services {
        project.all_warnings()
    } else {
        Vec::new()
    };
    let stdout = if list_services {
        let rendered = project.render_service_names();
        if rendered.is_empty() {
            String::new()
        } else {
            format!("{rendered}\n")
        }
    } else {
        project.render()?
    };
    Ok(RenderedComposeProject { stdout, warnings })
}
