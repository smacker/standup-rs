use std::fmt;

#[derive(Clone)]
pub struct Entry {
    pub r#type: String,
    pub title: String,
    pub url: String,
    pub actions: Vec<String>,
}

impl fmt::Display for Entry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "[{}] ({}) {} {}",
            self.r#type,
            self.actions.join(", "),
            self.title,
            self.url
        )
    }
}
