use c2pa::{Error, Reader, ValidationState};
use serde::Serialize;
use serde_json::Value;
use std::{collections::BTreeSet, path::Path};

const MAX_ITEMS: usize = 20;
const MAX_ITEM_CHARS: usize = 320;
const MAX_TOTAL_CHARS: usize = 4_000;

#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct C2paDetails {
    pub validation_result: String,
    pub generator: String,
    pub software_version: String,
    pub signer_issuer: String,
    pub signed_at: String,
    pub actions_history: String,
    pub ai_disclosure: String,
    pub manifest_id: String,
    pub specification_version: String,
    pub ingredients: String,
    pub validation_messages: String,
}

impl C2paDetails {
    fn absent() -> Self {
        Self {
            validation_result: "証明情報なし".to_string(),
            ai_disclosure: "申告なし（C2PA Manifestなし）".to_string(),
            validation_messages: "C2PA Manifestは見つかりませんでした。".to_string(),
            ..Self::default()
        }
    }

    fn failed(error: &Error) -> Self {
        Self {
            validation_result: "検証できません".to_string(),
            ai_disclosure: "確認できません".to_string(),
            validation_messages: truncate(&error.to_string(), MAX_ITEM_CHARS),
            ..Self::default()
        }
    }
}

pub fn inspect(path: &Path) -> C2paDetails {
    let reader = match Reader::default().with_file(path) {
        Ok(reader) => reader,
        Err(error) => {
            return match error {
                Error::JumbfNotFound | Error::JumbfBoxNotFound | Error::NotFound => {
                    C2paDetails::absent()
                }
                _ => C2paDetails::failed(&error),
            }
        }
    };

    let value = match serde_json::from_str::<Value>(&reader.json()) {
        Ok(value) => value,
        Err(error) => {
            return C2paDetails {
                validation_result: validation_result(reader.validation_state()),
                ai_disclosure: "確認できません".to_string(),
                validation_messages: format!("Manifestの表示データを解析できません: {error}"),
                ..C2paDetails::default()
            }
        }
    };

    let mut details = details_from_value(&value, reader.validation_state());
    details.validation_messages = validation_messages(&reader);
    details
}

fn details_from_value(root: &Value, state: ValidationState) -> C2paDetails {
    let manifest_id = root
        .get("active_manifest")
        .or_else(|| root.get("activeManifest"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let active = root
        .get("manifests")
        .and_then(Value::as_object)
        .and_then(|manifests| manifests.get(manifest_id))
        .or_else(|| {
            root.get("active_manifest")
                .filter(|value| value.is_object())
        })
        .unwrap_or(&Value::Null);

    let info = active
        .get("claim_generator_info")
        .or_else(|| active.get("claimGeneratorInfo"))
        .and_then(Value::as_array);
    let mut generators = Vec::new();
    let mut versions = Vec::new();
    let mut specifications = Vec::new();
    if let Some(entries) = info {
        for entry in entries.iter().take(MAX_ITEMS) {
            push_string_member(&mut generators, entry, &["name"]);
            push_string_member(&mut versions, entry, &["version"]);
            push_string_member(&mut specifications, entry, &["specVersion", "spec_version"]);
        }
    }
    if generators.is_empty() {
        push_string_member(
            &mut generators,
            active,
            &["claim_generator", "claimGenerator"],
        );
    }

    let mut devices = Vec::new();
    collect_devices(active, &mut devices);
    generators.extend(devices);

    let signature = active
        .get("signature_info")
        .or_else(|| active.get("signatureInfo"))
        .unwrap_or(&Value::Null);
    let signer = member_string(signature, &["common_name", "commonName"]);
    let issuer = member_string(signature, &["issuer"]);
    let signer_issuer = match (signer.as_deref(), issuer.as_deref()) {
        (Some(signer), Some(issuer)) if signer != issuer => {
            format!("署名者: {signer} / 証明書発行元: {issuer}")
        }
        (Some(signer), _) => format!("署名者: {signer}"),
        (_, Some(issuer)) => format!("証明書発行元: {issuer}"),
        _ => String::new(),
    };

    if specifications.is_empty() {
        push_string_member(
            &mut specifications,
            active,
            &["specVersion", "spec_version"],
        );
    }
    let specification_version = if specifications.is_empty() {
        member_string(active, &["claim_version", "claimVersion"])
            .map(|version| format!("不明（Claim v{version}）"))
            .unwrap_or_else(|| "不明".to_string())
    } else {
        join_unique(specifications, " / ")
    };

    let actions = collect_actions(active);
    let ai_sources = collect_ai_sources(active);
    let ai_disclosure = if ai_sources.is_empty() {
        "Manifest内にAI生成・AI編集の申告は見つかりません".to_string()
    } else {
        format!("申告あり: {}", join_unique(ai_sources, " / "))
    };

    let ingredients = collect_ingredients(active);

    C2paDetails {
        validation_result: validation_result(state),
        generator: join_unique(generators, " / "),
        software_version: join_unique(versions, " / "),
        signer_issuer,
        signed_at: member_string(signature, &["time", "timestamp"]).unwrap_or_default(),
        actions_history: if actions.is_empty() {
            "作成・編集履歴の記録なし".to_string()
        } else {
            join_limited(actions)
        },
        ai_disclosure,
        manifest_id: if manifest_id.is_empty() {
            member_string(active, &["label", "instance_id", "instanceId"]).unwrap_or_default()
        } else {
            truncate(manifest_id, MAX_ITEM_CHARS)
        },
        specification_version,
        ingredients: if ingredients.is_empty() {
            "元素材・入力元の記録なし".to_string()
        } else {
            join_limited(ingredients)
        },
        validation_messages: String::new(),
    }
}

fn validation_result(state: ValidationState) -> String {
    match state {
        ValidationState::Trusted => "信頼済み（署名・内容・発行元を検証）",
        ValidationState::Valid => "有効（署名と内容を検証、発行元の信頼性は未確認）",
        ValidationState::Invalid => "検証失敗",
    }
    .to_string()
}

fn validation_messages(reader: &Reader) -> String {
    let mut messages = Vec::new();
    if let Some(results) = reader.validation_results() {
        if let Ok(value) = serde_json::to_value(results) {
            collect_status_group(&value, "failure", "エラー", &mut messages);
            collect_status_group(&value, "informational", "警告・情報", &mut messages);
        }
    }
    if let Some(statuses) = reader.validation_status() {
        for status in statuses.iter().take(MAX_ITEMS) {
            let mut message = format!("エラー: {}", status.code());
            if let Some(explanation) = status.explanation() {
                message.push_str(": ");
                message.push_str(explanation);
            }
            push_unique(&mut messages, truncate(&message, MAX_ITEM_CHARS));
        }
    }
    if messages.is_empty() {
        "警告・エラーは報告されていません".to_string()
    } else {
        join_limited(messages)
    }
}

fn collect_status_group(value: &Value, group: &str, prefix: &str, output: &mut Vec<String>) {
    if output.len() >= MAX_ITEMS {
        return;
    }
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if key == group {
                    if let Some(items) = child.as_array() {
                        for item in items.iter().take(MAX_ITEMS - output.len()) {
                            let code = member_string(item, &["code"]).unwrap_or_default();
                            let explanation = member_string(item, &["explanation"]);
                            let message = match explanation {
                                Some(explanation) if !explanation.is_empty() => {
                                    format!("{prefix}: {code}: {explanation}")
                                }
                                _ => format!("{prefix}: {code}"),
                            };
                            push_unique(output, truncate(&message, MAX_ITEM_CHARS));
                        }
                    }
                } else {
                    collect_status_group(child, group, prefix, output);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_status_group(item, group, prefix, output);
            }
        }
        _ => {}
    }
}

fn collect_actions(active: &Value) -> Vec<String> {
    let mut output = Vec::new();
    let assertions = active
        .get("assertions")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    for assertion in assertions {
        let label = member_string(assertion, &["label"]).unwrap_or_default();
        if !label.starts_with("c2pa.actions") {
            continue;
        }
        let actions = assertion
            .get("data")
            .and_then(|data| data.get("actions"))
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        for (index, action) in actions.iter().take(MAX_ITEMS).enumerate() {
            let action_name =
                member_string(action, &["action"]).unwrap_or_else(|| "不明".to_string());
            let mut parts = vec![format!("{}. {action_name}", index + 1)];
            if let Some(agent) = action
                .get("softwareAgent")
                .or_else(|| action.get("software_agent"))
            {
                if let Some(agent) = value_label(agent) {
                    parts.push(agent);
                }
            }
            if let Some(when) = member_string(action, &["when"]) {
                parts.push(when);
            }
            push_unique(&mut output, truncate(&parts.join(" / "), MAX_ITEM_CHARS));
        }
    }
    output
}

fn collect_ingredients(active: &Value) -> Vec<String> {
    let mut output = Vec::new();
    let ingredients = active
        .get("ingredients")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    for ingredient in ingredients.iter().take(MAX_ITEMS) {
        let mut parts = Vec::new();
        for keys in [
            &["title"][..],
            &["format"][..],
            &["relationship"][..],
            &["instance_id", "instanceId"][..],
        ] {
            if let Some(value) = member_string(ingredient, keys) {
                push_unique(&mut parts, value);
            }
        }
        if !parts.is_empty() {
            push_unique(&mut output, truncate(&parts.join(" / "), MAX_ITEM_CHARS));
        }
    }
    output
}

fn collect_devices(active: &Value, output: &mut Vec<String>) {
    let assertions = active
        .get("assertions")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    for assertion in assertions {
        let label = member_string(assertion, &["label"]).unwrap_or_default();
        if !label.to_ascii_lowercase().contains("exif") {
            continue;
        }
        if let Some(data) = assertion.get("data") {
            let make = find_recursive_string(data, &["Make", "make"]);
            let model = find_recursive_string(data, &["Model", "model"]);
            match (make, model) {
                (Some(make), Some(model)) => push_unique(output, format!("{make} {model}")),
                (Some(make), None) => push_unique(output, make),
                (None, Some(model)) => push_unique(output, model),
                _ => {}
            }
        }
    }
}

fn collect_ai_sources(value: &Value) -> Vec<String> {
    let mut output = Vec::new();
    collect_ai_sources_inner(value, &mut output);
    output
}

fn collect_ai_sources_inner(value: &Value, output: &mut Vec<String>) {
    if output.len() >= MAX_ITEMS {
        return;
    }
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let key_lower = key.to_ascii_lowercase();
                if matches!(
                    key_lower.as_str(),
                    "digitalsourcetype" | "digital_source_type"
                ) {
                    if let Some(text) = value_label(child) {
                        if is_ai_marker(&text) {
                            push_unique(output, truncate(&text, MAX_ITEM_CHARS));
                        }
                    }
                }
                collect_ai_sources_inner(child, output);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_ai_sources_inner(item, output);
            }
        }
        Value::String(text) if is_ai_marker(text) => {
            push_unique(output, truncate(text, MAX_ITEM_CHARS));
        }
        _ => {}
    }
}

fn is_ai_marker(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "trainedalgorithmic",
        "generative",
        "synthetic",
        "algorithmicmedia",
        "ai-generated",
        "ai generated",
        "ai-edited",
        "ai edited",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

fn find_recursive_string(value: &Value, keys: &[&str]) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(found) = map.get(*key).and_then(value_label) {
                    return Some(found);
                }
            }
            map.values()
                .find_map(|child| find_recursive_string(child, keys))
        }
        Value::Array(items) => items
            .iter()
            .find_map(|child| find_recursive_string(child, keys)),
        _ => None,
    }
}

fn push_string_member(output: &mut Vec<String>, value: &Value, keys: &[&str]) {
    if let Some(value) = member_string(value, keys) {
        push_unique(output, value);
    }
}

fn member_string(value: &Value, keys: &[&str]) -> Option<String> {
    let map = value.as_object()?;
    keys.iter()
        .find_map(|key| map.get(*key).and_then(value_label))
}

fn value_label(value: &Value) -> Option<String> {
    match value {
        Value::String(text) if !text.trim().is_empty() => Some(text.trim().to_string()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(boolean) => Some(boolean.to_string()),
        Value::Object(_) => {
            let name = member_string(value, &["name"])?;
            let version = member_string(value, &["version"]);
            Some(match version {
                Some(version) => format!("{name} {version}"),
                None => name,
            })
        }
        _ => None,
    }
}

fn push_unique(output: &mut Vec<String>, value: String) {
    if !value.is_empty() && output.len() < MAX_ITEMS && !output.contains(&value) {
        output.push(value);
    }
}

fn join_unique(values: Vec<String>, separator: &str) -> String {
    let mut unique = BTreeSet::new();
    let ordered = values
        .into_iter()
        .filter(|value| unique.insert(value.clone()))
        .collect::<Vec<_>>();
    truncate(&ordered.join(separator), MAX_TOTAL_CHARS)
}

fn join_limited(values: Vec<String>) -> String {
    truncate(&values.join("\n"), MAX_TOTAL_CHARS)
}

fn truncate(value: &str, maximum: usize) -> String {
    let count = value.chars().count();
    if count <= maximum {
        return value.to_string();
    }
    let mut truncated = value
        .chars()
        .take(maximum.saturating_sub(1))
        .collect::<String>();
    truncated.push('…');
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_requested_c2pa_fields() {
        let value = json!({
            "active_manifest": "urn:c2pa:manifest:test",
            "manifests": {
                "urn:c2pa:manifest:test": {
                    "claim_generator": "QuickVideoMaker/1.7.0",
                    "claim_generator_info": [{
                        "name": "QuickVideoMaker",
                        "version": "1.7.0",
                        "specVersion": "2.2"
                    }],
                    "claim_version": 2,
                    "ingredients": [{
                        "title": "source.mp4",
                        "format": "video/mp4",
                        "relationship": "parentOf",
                        "instance_id": "xmp:iid:source"
                    }],
                    "assertions": [
                        {
                            "label": "c2pa.actions.v2",
                            "data": {"actions": [{
                                "action": "c2pa.edited",
                                "softwareAgent": {"name": "QuickVideoMaker", "version": "1.7.0"},
                                "when": "2026-08-30T12:00:00+09:00",
                                "digitalSourceType": "http://cv.iptc.org/newscodes/digitalsourcetype/trainedAlgorithmicMedia"
                            }]}
                        },
                        {
                            "label": "stds.exif",
                            "data": {"Make": "Example", "Model": "Camera X"}
                        }
                    ],
                    "signature_info": {
                        "issuer": "Example CA",
                        "common_name": "Example Signer",
                        "time": "2026-08-30T03:00:00Z"
                    }
                }
            }
        });

        let details = details_from_value(&value, ValidationState::Valid);
        assert!(details.validation_result.starts_with("有効"));
        assert!(details.generator.contains("QuickVideoMaker"));
        assert!(details.generator.contains("Example Camera X"));
        assert_eq!(details.software_version, "1.7.0");
        assert!(details.signer_issuer.contains("Example Signer"));
        assert!(details.signer_issuer.contains("Example CA"));
        assert_eq!(details.signed_at, "2026-08-30T03:00:00Z");
        assert!(details.actions_history.contains("c2pa.edited"));
        assert!(details.ai_disclosure.starts_with("申告あり"));
        assert_eq!(details.manifest_id, "urn:c2pa:manifest:test");
        assert_eq!(details.specification_version, "2.2");
        assert!(details.ingredients.contains("source.mp4"));
    }

    #[test]
    fn reports_missing_optional_c2pa_records() {
        let value = json!({
            "active_manifest": "urn:c2pa:manifest:minimal",
            "manifests": {
                "urn:c2pa:manifest:minimal": {"claim_version": 1}
            }
        });
        let details = details_from_value(&value, ValidationState::Invalid);
        assert_eq!(details.validation_result, "検証失敗");
        assert_eq!(details.actions_history, "作成・編集履歴の記録なし");
        assert!(details.ai_disclosure.contains("見つかりません"));
        assert_eq!(details.specification_version, "不明（Claim v1）");
        assert_eq!(details.ingredients, "元素材・入力元の記録なし");
    }

    #[test]
    fn reports_unsigned_file_when_fixture_is_available() {
        let Some(path) = std::env::var_os("QVM_UNSIGNED_TEST_FILE") else {
            return;
        };
        let details = inspect(Path::new(&path));
        assert_eq!(details.validation_result, "証明情報なし");
    }

    #[test]
    fn inspects_official_signed_fixture_when_available() {
        let Some(path) = std::env::var_os("QVM_C2PA_TEST_FILE") else {
            return;
        };
        let details = inspect(Path::new(&path));
        assert_ne!(details.validation_result, "証明情報なし");
        assert_ne!(details.validation_result, "検証できません");
        assert!(!details.manifest_id.is_empty());
        assert!(!details.generator.is_empty());
    }
}
