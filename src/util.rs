pub fn extract_callsign(nick: &String) -> Option<String> {
    // Split by spaces, if any tokens are:
    //  Longer than 2 characters
    //  All uppercase including numbers
    //  Ends with a letter
    // Then return callsign
    nick.split_whitespace()
        .filter(|x| {
            x.len() > 2
                && x.chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
                && x.chars().last().map(|c| c.is_ascii_alphabetic()).unwrap_or(false)
        })
        .map(|x| x.to_string())
        .next()
}