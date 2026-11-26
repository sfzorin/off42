//! Shell-neutral requests for user input; each shell renders them with its own controls.

/// A titled form of labelled text fields with their initial values.
#[derive(Clone)]
pub struct InputRequest {
    pub title: &'static str,
    pub fields: Vec<(&'static str, String)>,
}
