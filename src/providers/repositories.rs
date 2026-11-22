use super::*;

const REPOS: &str = "repositories";
pub(super) const REPOSITORIES: Field = Field {
    required: true,
    max_length: Some(140),
    max: Some(8),
    ..Field::new(
        REPOS,
        "Repositories",
        Kind::List,
        "Enter up to eight owner/repository names, one per line.",
    )
};

pub(super) fn names(options: &Values) -> Result<Vec<&str>, &'static str> {
    let value = options.get(REPOS).ok_or(REPOS)?;
    if !REPOSITORIES.accepts(value) {
        return Err(REPOS);
    }
    let mut names: Vec<&str> = Vec::new();
    for name in value.as_array().ok_or(REPOS)? {
        let name = name.as_str().ok_or(REPOS)?;
        let (owner, repo) = name.split_once('/').ok_or(REPOS)?;
        if [owner, repo].iter().any(|part| {
            part.is_empty()
                || matches!(*part, "." | "..")
                || !part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        }) || names
            .iter()
            .any(|previous| previous.eq_ignore_ascii_case(name))
        {
            return Err(REPOS);
        }
        names.push(name);
    }
    Ok(names)
}

pub(super) fn validate_options(options: &Values) -> Result<(), &'static str> {
    names(options).map(|_| ())
}
