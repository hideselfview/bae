use crate::db::DbRelease;

/// Format a release name for display in menus
pub fn format_release_display(release: &DbRelease) -> String {
    if let Some(name) = &release.release_name {
        name.clone()
    } else if let Some(year) = release.year {
        format!("Release ({})", year)
    } else {
        "Release".to_string()
    }
}
