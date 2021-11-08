use reqwest::Url;
use regex::Regex;

pub fn is_url_or_ssh_repo(val: String) -> Result<(), String> {
    if let Ok(_) = val.parse::<Url>() {
        return Ok(())
    }
    if is_ssh_repo(val) {
        Ok(())
    } else {
        Err("the given path does not specify a valid remote repo.".to_owned())
    }
}

fn is_ssh_repo(val: String) -> bool {
    let re = Regex::new(r"^(ssh://\w+@\w+/|\w+@\w+:)\w+(/\w+)*(.git)?$").expect("ssh regex was invalid");
    re.is_match(&val)
}

