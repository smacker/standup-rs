use std::fmt;

#[derive(Clone)]
pub struct Entry {
    pub r#type: String,
    pub title: String,
    pub url: Option<String>,
    pub actions: Vec<String>,
}

impl fmt::Display for Entry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let blank = "".to_string();
        let url = self.url.as_ref().unwrap_or(&blank);
        write!(f, "[{}] ", self.r#type)?;
        if !self.actions.is_empty() {
            write!(f, "({}) ", self.actions.join(", "))?;
        }
        write!(f, "{} {}", self.title, url)
    }
}
