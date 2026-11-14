use serde_json::{Value, json};

pub fn pending(ticket: &Value) -> bool {
    ticket["preparation"].is_object() && ticket["preparation"]["published"] != true
}

pub fn update(ticket: &mut Value, udid: &str, operation: &str, receipt: &Value, now: f64) -> bool {
    let Some(installed) = ticket["installedAt"].as_f64() else { return false };
    if ticket["state"] != "active" || !ticket["preparation"].is_object()
        || receipt["ticketID"] != ticket["id"] || receipt["bundleID"] != ticket["bundleId"]
        || receipt["udid"] != udid || now >= installed + 90.0 { return false }
    let prep = &mut ticket["preparation"];
    if prep.get("failure").is_some() { return false }
    let tuple = json!([receipt["primaryUserID"], receipt["pid"], receipt["bootID"], receipt["epoch"]]);
    let valid = |value: &Value| value.as_str().is_some_and(|s| !s.is_empty() && s.len() <= 128);
    match operation {
        "delivered" | "ack" => {
            if !valid(&tuple[0]) || !valid(&tuple[2]) || !tuple[1].as_u64().is_some_and(|pid| pid > 0)
                || tuple[3].as_u64().is_none() { return false }
            if operation == "delivered" {
                if let Some(old) = prep.get("tuple") { return old == &tuple }
                prep["tuple"] = tuple;
                prep["deliveredAt"] = json!(now);
                true
            } else {
                if prep["tuple"] != tuple || receipt["fixtureInputsReady"] != true || receipt["manualPanelReady"] != true { return false }
                let Some(timestamp) = receipt["readyAt"].as_f64() else { return false };
                if prep["readyAt"].as_f64() == Some(timestamp) { return true }
                if prep.get("readyAt").is_some() || timestamp < prep["deliveredAt"].as_f64().unwrap_or(now)
                    || timestamp > now || now - timestamp > 15.0 { return false }
                prep["readyAt"] = json!(timestamp);
                true
            }
        }
        "fail" if prep.get("readyAt").is_none() => {
            let safe_token = |value: &Value| valid(value) && value.as_str().unwrap().bytes().all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c));
            if !safe_token(&receipt["phase"]) || !(receipt["exitCode"].is_null() || receipt["exitCode"].as_i64().is_some()) { return false }
            let Some(errors) = receipt["errors"].as_array() else { return false };
            if errors.len() > 9 || errors.iter().any(|e| !safe_token(&e["domain"]) || e["code"].as_i64().is_none()) { return false }
            prep["failure"] = json!({"phase":receipt["phase"],"exitCode":receipt["exitCode"],
                "errors":errors.iter().map(|e|json!({"domain":e["domain"],"code":e["code"]})).collect::<Vec<_>>()});
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guarded_preparation_is_one_ticket() {
        let mut t = json!({"id":"1114-155736-206-Тест","bundleId":"app.test","state":"active",
            "installedAt":100.0,"preparation":{},"items":[{"label":"1","text":"test"}]});
        let r = json!({"ticketID":t["id"],"bundleID":t["bundleId"],"udid":"device",
            "primaryUserID":"user","pid":3,"bootID":"boot","epoch":1,"fixtureInputsReady":true,
            "manualPanelReady":true,"readyAt":106.0});
        assert!(pending(&t));
        assert!(!update(&mut t,"device","ack",&r,106.0));
        assert!(update(&mut t,"device","delivered",&r,105.0));
        for (key,bad) in [("ticketID",json!("foreign")),("udid",json!("other")),("bundleID",json!("foreign")),
            ("pid",json!(4)),("bootID",json!("different")),("primaryUserID",json!("other")),
            ("epoch",json!(2)),("readyAt",json!(104.0)),("manualPanelReady",json!(false))] {
            let mut wrong=r.clone();wrong[key]=bad;
            assert!(!update(&mut t,"device","ack",&wrong,107.0), "{key}");
        }
        assert!(!update(&mut t,"device","ack",&r,189.0));
        assert!(update(&mut t,"device","ack",&r,107.0));
        assert!(update(&mut t,"device","ack",&r,108.0));
        assert!(pending(&t)); // Until the dispatcher has sent the phone permission.
        t["preparation"]["published"]=json!(true);
        assert!(!pending(&t));
        assert_eq!(t["items"][0]["label"],"1");
        t["state"]=json!("cancelled");
        assert!(!update(&mut t,"device","ack",&r,109.0));
    }

    #[test]
    fn failure_keeps_only_safe_fields() {
        let mut t=json!({"id":"check","bundleId":"app.test","state":"active","installedAt":100.0,"preparation":{}});
        let r=json!({"ticketID":"check","bundleID":"app.test","udid":"device","phase":"process-list","exitCode":1,
            "errors":[{"domain":"NSPOSIXErrorDomain","code":13,"underlying":"private value"}],"stderr":"private value"});
        assert!(update(&mut t,"device","fail",&r,101.0));
        assert!(!t.to_string().contains("private value"));
        assert!(!update(&mut t,"device","fail",&r,102.0));
    }
}
