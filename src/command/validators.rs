use hyper::Url;
use regex::Regex;

pub fn is_url(val: String) -> Result<(), String> {
    Url::parse(&val)
        .map(|_| ())
        .map_err(|_| "the given path must be a valid URL.".to_owned())
}

pub fn is_ssh_repo(val: String) -> Result<(), String> {
    let re = Regex::new(r"^(ssh://\w+@\w+/|\w+@\w+:)\w+(/\w+)*(.git)?$").unwrap();
    if re.is_match(&val) {
        Ok(())
    } else {
        Err("the given path does not specify a valid remote repo.".to_owned())
    }
}

pub fn is_url_or_ssh_repo(val: String) -> Result<(), String> {
    is_url(val).or_else(is_ssh_repo)
}
