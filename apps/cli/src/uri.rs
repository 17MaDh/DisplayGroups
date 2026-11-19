//! Strict parser for `displaygroups://` links (ARCHITECTURE.md §11.2). Only two forms are
//! accepted and every id must be a plain hyphenated UUID; anything else is rejected.

use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UriCommand {
    Apply(Uuid),
    Toggle(Vec<Uuid>),
}

const SCHEME: &str = "displaygroups://";

pub fn parse_uri(url: &str) -> Result<UriCommand, String> {
    let url = url.trim();
    let rest = url
        .get(..SCHEME.len())
        .filter(|s| s.eq_ignore_ascii_case(SCHEME))
        .map(|_| &url[SCHEME.len()..])
        .ok_or_else(|| format!("not a {SCHEME} link"))?;
    let rest = rest.trim_end_matches('/');
    let (verb, arg) = rest
        .split_once('/')
        .ok_or_else(|| "the link has no group id".to_string())?;
    match verb.to_ascii_lowercase().as_str() {
        "apply" => Ok(UriCommand::Apply(parse_id(arg)?)),
        "toggle" => {
            let arg = arg.replace("%2C", ",").replace("%2c", ",");
            let ids = arg
                .split(',')
                .map(parse_id)
                .collect::<Result<Vec<_>, _>>()?;
            if ids.len() < 2 {
                return Err("toggle needs at least two group ids".into());
            }
            Ok(UriCommand::Toggle(ids))
        }
        other => Err(format!("unknown action \"{other}\"")),
    }
}

fn parse_id(s: &str) -> Result<Uuid, String> {
    let s = s.trim();
    let plain = s.len() == 36 && s.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    if !plain {
        return Err(format!("\"{s}\" is not a group id"));
    }
    Uuid::parse_str(s).map_err(|_| format!("\"{s}\" is not a group id"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "6b0f2c1e-8a7d-4c1e-9f0a-1b2c3d4e5f60";
    const B: &str = "5d3f0e2a-7c41-4b8e-9a10-00000000d35c";

    #[test]
    fn accepts_the_two_documented_forms() {
        assert_eq!(
            parse_uri(&format!("displaygroups://apply/{A}")),
            Ok(UriCommand::Apply(Uuid::parse_str(A).unwrap()))
        );
        assert_eq!(
            parse_uri(&format!("DisplayGroups://toggle/{A},{B}/")),
            Ok(UriCommand::Toggle(vec![
                Uuid::parse_str(A).unwrap(),
                Uuid::parse_str(B).unwrap()
            ]))
        );
        assert!(parse_uri(&format!("displaygroups://toggle/{A}%2C{B}")).is_ok());
    }

    #[test]
    fn rejects_everything_else() {
        for bad in [
            "https://example.com",
            "displaygroups://",
            "displaygroups://apply/",
            "displaygroups://apply/Desktop",
            &format!("displaygroups://apply/{A}?cmd=calc"),
            &format!("displaygroups://apply/{{{A}}}"),
            &format!("displaygroups://toggle/{A}"),
            &format!("displaygroups://run/{A}"),
        ] {
            assert!(parse_uri(bad).is_err(), "{bad} should be rejected");
        }
    }
}
