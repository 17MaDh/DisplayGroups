//! Tiered matching of a remembered [`MonitorIdentity`] against connected monitors.
//!
//! Tiers, strongest first. A tier is only used if it yields exactly one candidate:
//! 1. manufacturer + product + serial
//! 2. manufacturer + product + connector (identical twins without serials)
//! 3. manufacturer + product (the cable moved to another port)
//! 4. friendly name (weak; e.g. after a firmware update changed the product code)
//!
//! A candidate whose serial is known and differs from the remembered serial is never
//! matched by tiers 2–4: it is a different unit of the same model.

use dg_model::MonitorIdentity;
use serde::Serialize;

/// What we know about a currently connected (available) monitor.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Observed {
    pub pnp_id: String,
    pub product_code: String,
    pub serial: Option<String>,
    pub friendly_name: String,
    pub connector_hint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MatchTier {
    Serial,
    Connector,
    Product,
    FriendlyName,
}

impl MatchTier {
    /// Tiers 1–3 identify the model reliably; tier 4 is a guess worth flagging.
    pub fn is_certain(self) -> bool {
        self != MatchTier::FriendlyName
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Matched {
    /// Index into the candidate slice.
    pub index: usize,
    pub tier: MatchTier,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchError {
    NotFound,
    /// Several candidates fit equally well at the best tier that had any hit.
    Ambiguous {
        tier: MatchTier,
        count: usize,
    },
}

pub fn match_monitor(id: &MonitorIdentity, candidates: &[Observed]) -> Result<Matched, MatchError> {
    let id_serial = clean(id.serial.as_deref());
    let same_model = |c: &Observed| {
        c.pnp_id.eq_ignore_ascii_case(&id.pnp_id)
            && c.product_code.eq_ignore_ascii_case(&id.product_code)
    };
    let serial_compatible = |c: &Observed| match (id_serial, clean(c.serial.as_deref())) {
        (Some(a), Some(b)) => a == b,
        _ => true,
    };

    let mut ambiguous: Option<MatchError> = None;
    let mut decide = |tier: MatchTier, hits: Vec<usize>| -> Option<Matched> {
        match hits.len() {
            1 => Some(Matched {
                index: hits[0],
                tier,
            }),
            0 => None,
            count => {
                ambiguous.get_or_insert(MatchError::Ambiguous { tier, count });
                None
            }
        }
    };
    let hits = |pred: &dyn Fn(&Observed) -> bool| -> Vec<usize> {
        candidates
            .iter()
            .enumerate()
            .filter(|(_, c)| pred(c))
            .map(|(i, _)| i)
            .collect()
    };

    if let Some(serial) = id_serial {
        let h = hits(&|c| same_model(c) && clean(c.serial.as_deref()) == Some(serial));
        // More than one unit reporting the same serial means a lazy EDID: fall through.
        if h.len() == 1 {
            return Ok(Matched {
                index: h[0],
                tier: MatchTier::Serial,
            });
        }
    }

    if let Some(conn) = clean(id.connector_hint.as_deref()) {
        let h = hits(&|c| {
            same_model(c)
                && serial_compatible(c)
                && clean(c.connector_hint.as_deref()).is_some_and(|x| x.eq_ignore_ascii_case(conn))
        });
        if let Some(m) = decide(MatchTier::Connector, h) {
            return Ok(m);
        }
    }

    let h = hits(&|c| same_model(c) && serial_compatible(c));
    if let Some(m) = decide(MatchTier::Product, h) {
        return Ok(m);
    }

    if let Some(name) = clean(Some(id.friendly_name.as_str())) {
        let h =
            hits(&|c| c.friendly_name.trim().eq_ignore_ascii_case(name) && serial_compatible(c));
        if let Some(m) = decide(MatchTier::FriendlyName, h) {
            return Ok(m);
        }
    }

    Err(ambiguous.unwrap_or(MatchError::NotFound))
}

fn clean(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(pnp: &str, product: &str, serial: Option<&str>, name: &str, conn: &str) -> Observed {
        Observed {
            pnp_id: pnp.into(),
            product_code: product.into(),
            serial: serial.map(Into::into),
            friendly_name: name.into(),
            connector_hint: Some(conn.into()),
        }
    }

    fn ident(
        pnp: &str,
        product: &str,
        serial: Option<&str>,
        name: &str,
        conn: Option<&str>,
    ) -> MonitorIdentity {
        MonitorIdentity {
            pnp_id: pnp.into(),
            product_code: product.into(),
            serial: serial.map(Into::into),
            friendly_name: name.into(),
            connector_hint: conn.map(Into::into),
            ..Default::default()
        }
    }

    /// The three displays of the test machine, as CCD reports them (no serials via CCD).
    fn rig() -> Vec<Observed> {
        vec![
            obs("SAM", "721B", None, "SAMSUNG", "UID4352"),
            obs("HXM", "40F3", None, "DFI27280", "UID4353"),
            obs("AUS", "AA06", None, "XG32UCWMG", "UID4355"),
        ]
    }

    #[test]
    fn rig_monitors_match_by_model() {
        let tv = ident("SAM", "721B", None, "SAMSUNG", Some("UID4352"));
        assert_eq!(
            match_monitor(&tv, &rig()),
            Ok(Matched {
                index: 0,
                tier: MatchTier::Connector
            })
        );
        let asus = ident("AUS", "AA06", None, "XG32UCWMG", None);
        assert_eq!(
            match_monitor(&asus, &rig()),
            Ok(Matched {
                index: 2,
                tier: MatchTier::Product
            })
        );
    }

    #[test]
    fn serial_wins_after_a_port_change() {
        let cands = vec![
            obs("DEL", "A0B1", Some("AAA111"), "DELL U2723", "UID9000"),
            obs("DEL", "A0B1", Some("BBB222"), "DELL U2723", "UID9001"),
        ];
        // Remembered on UID9001, now plugged into UID9000.
        let id = ident("DEL", "A0B1", Some("AAA111"), "DELL U2723", Some("UID9001"));
        assert_eq!(
            match_monitor(&id, &cands),
            Ok(Matched {
                index: 0,
                tier: MatchTier::Serial
            })
        );
    }

    #[test]
    fn identical_twins_without_serials_use_the_connector() {
        let cands = vec![
            obs("GSM", "5B7F", None, "LG ULTRAGEAR", "UID256"),
            obs("GSM", "5B7F", None, "LG ULTRAGEAR", "UID257"),
        ];
        let right = ident("GSM", "5B7F", None, "LG ULTRAGEAR", Some("UID257"));
        assert_eq!(
            match_monitor(&right, &cands),
            Ok(Matched {
                index: 1,
                tier: MatchTier::Connector
            })
        );
        let unknown_port = ident("GSM", "5B7F", None, "LG ULTRAGEAR", None);
        assert_eq!(
            match_monitor(&unknown_port, &cands),
            Err(MatchError::Ambiguous {
                tier: MatchTier::Product,
                count: 2
            })
        );
    }

    #[test]
    fn another_unit_of_the_same_model_is_not_stolen() {
        let cands = vec![obs("DEL", "A0B1", Some("BBB222"), "DELL U2723", "UID9000")];
        let id = ident("DEL", "A0B1", Some("AAA111"), "DELL U2723", Some("UID9000"));
        assert_eq!(match_monitor(&id, &cands), Err(MatchError::NotFound));
    }

    #[test]
    fn friendly_name_is_the_last_resort() {
        // Firmware update changed the product code.
        let cands = vec![obs("SAM", "7300", None, "SAMSUNG", "UID4352")];
        let id = ident("SAM", "721B", None, "SAMSUNG", None);
        let m = match_monitor(&id, &cands).unwrap();
        assert_eq!(m.tier, MatchTier::FriendlyName);
        assert!(!m.tier.is_certain());
    }

    #[test]
    fn missing_monitor_is_not_found() {
        let id = ident("BNQ", "7F2C", None, "BenQ", None);
        assert_eq!(match_monitor(&id, &rig()), Err(MatchError::NotFound));
    }
}
