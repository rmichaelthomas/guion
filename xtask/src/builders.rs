use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path};

const CLASSES: [&str; 3] = [
    "small-automation-cli",
    "concurrent-network",
    "maintainable-local-app",
];
const LANGUAGES: [&str; 2] = ["python", "veritas"];
const REEVALUATION_BRIEF: &str = "field-kit-log";

fn read_json(path: &Path) -> Result<Value, String> {
    let contents =
        fs::read_to_string(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    serde_json::from_str(&contents).map_err(|error| format!("parse {}: {error}", path.display()))
}

fn required_text<'a>(value: &'a Value, field: &str) -> Result<&'a str, String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| format!("{field} must be non-empty text"))
}

fn required_bool(value: &Value, field: &str) -> Result<bool, String> {
    value
        .get(field)
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("{field} must be a boolean"))
}

fn safe_relative_path(value: &str) -> Result<(), String> {
    let path = Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(format!("unsafe report path: {value}"));
    }
    Ok(())
}

fn validate_builder_report(report: &Value, assignment_id: &str) -> Result<(), String> {
    if report.get("schema_version").and_then(Value::as_u64) != Some(1) {
        return Err(format!("{assignment_id}: schema_version must be 1"));
    }
    if required_text(report, "assignment_id")? != assignment_id {
        return Err(format!(
            "{assignment_id}: report assignment_id does not match"
        ));
    }
    if required_text(report, "status")? != "complete" {
        return Err(format!("{assignment_id}: builder status is not complete"));
    }
    for field in ["language", "brief_id", "start_utc", "finish_utc"] {
        required_text(report, field)?;
    }
    if report.get("budget_minutes").and_then(Value::as_u64) != Some(15) {
        return Err(format!("{assignment_id}: report budget_minutes must be 15"));
    }
    if report
        .get("elapsed_seconds")
        .and_then(Value::as_f64)
        .is_none_or(|seconds| !(0.0..=900.0).contains(&seconds))
    {
        return Err(format!(
            "{assignment_id}: elapsed_seconds must be within the 15-minute budget"
        ));
    }
    for field in ["initial_correct", "followup_correct", "relocation_passed"] {
        if !required_bool(report, field)? {
            let label = match field {
                "initial_correct" => "initial correctness",
                "followup_correct" => "follow-up correctness",
                _ => "relocation",
            };
            return Err(format!("{assignment_id}: {label} check failed"));
        }
    }
    for field in [
        "setup_seconds",
        "build_seconds",
        "followup_seconds",
        "installed_launch_seconds",
        "artifact_bytes",
    ] {
        if report
            .get(field)
            .and_then(Value::as_f64)
            .is_none_or(|number| number < 0.0)
        {
            return Err(format!(
                "{assignment_id}: {field} must be a nonnegative number"
            ));
        }
    }
    for field in ["interventions", "failures"] {
        if report.get(field).and_then(Value::as_array).is_none() {
            return Err(format!("{assignment_id}: {field} must be an array"));
        }
    }
    let recommendation = report
        .get("recommendation")
        .and_then(Value::as_object)
        .ok_or_else(|| format!("{assignment_id}: recommendation must be an object"))?;
    for field in ["preferred_language", "reason", "observed_user_benefit"] {
        if recommendation
            .get(field)
            .and_then(Value::as_str)
            .is_none_or(|text| text.trim().is_empty())
        {
            return Err(format!(
                "{assignment_id}: recommendation.{field} must be non-empty text"
            ));
        }
    }
    if recommendation
        .get("material_regressions")
        .and_then(Value::as_array)
        .is_none()
    {
        return Err(format!(
            "{assignment_id}: recommendation.material_regressions must be an array"
        ));
    }
    let evidence = report
        .get("evidence_paths")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{assignment_id}: evidence_paths must be an array"))?;
    if evidence.is_empty() {
        return Err(format!("{assignment_id}: evidence_paths must not be empty"));
    }
    Ok(())
}

pub fn audit(root: &Path) -> Result<Vec<String>, String> {
    let builder_root = root.join("evaluation/completion/builder");
    let assignments = read_json(&builder_root.join("assignments.json"))?;
    if assignments.get("schema_version").and_then(Value::as_u64) != Some(1) {
        return Err("builder assignments schema_version must be 1".into());
    }
    let randomization = assignments
        .get("randomization")
        .and_then(Value::as_object)
        .ok_or("builder assignments must record randomization metadata")?;
    if randomization.get("seed").and_then(Value::as_u64) != Some(20261025)
        || randomization
            .get("algorithm")
            .and_then(Value::as_str)
            .is_none_or(|value| !value.contains("Python random.Random shuffle"))
    {
        return Err("builder pair order must use the pre-recorded Python shuffle seed".into());
    }
    let assignment_list = assignments
        .get("assignments")
        .and_then(Value::as_array)
        .ok_or("builder assignments must contain an assignments array")?;
    if assignment_list.len() != CLASSES.len() * LANGUAGES.len() {
        return Err("expected six fresh assignments across three classes".into());
    }

    let model = required_text(&assignments, "model")?;
    let effort = required_text(&assignments, "reasoning_effort")?;
    let mut seen_ids = BTreeSet::new();
    let mut seen_pairs = BTreeSet::new();
    let mut brief_assignments: BTreeMap<String, (String, Vec<(u64, String)>)> = BTreeMap::new();
    for assignment in assignment_list {
        let id = required_text(assignment, "assignment_id")?;
        if !seen_ids.insert(id.to_string()) {
            return Err(format!("duplicate assignment_id: {id}"));
        }
        let class = required_text(assignment, "target_class")?;
        let language = required_text(assignment, "language")?;
        let brief = required_text(assignment, "brief_id")?;
        if !CLASSES.contains(&class) || !LANGUAGES.contains(&language) {
            return Err(format!("{id}: unknown target class or language"));
        }
        if assignment.get("context").and_then(Value::as_str) != Some("fresh") {
            return Err(format!("{id}: context must be fresh"));
        }
        let private_directory = required_text(assignment, "private_work_directory")?;
        if !private_directory.starts_with("/private/tmp/veritas-fresh-builders/") {
            return Err(format!(
                "{id}: private_work_directory must be under the fresh-builder root"
            ));
        }
        if assignment
            .get("budget_minutes_including_followup")
            .and_then(Value::as_u64)
            != Some(15)
        {
            return Err(format!("{id}: builder budget must be 15 minutes"));
        }
        if assignment
            .get("model")
            .and_then(Value::as_str)
            .is_some_and(|value| value != model)
            || assignment
                .get("reasoning_effort")
                .and_then(Value::as_str)
                .is_some_and(|value| value != effort)
        {
            return Err(format!(
                "{id}: model and reasoning effort must match the recorded configuration"
            ));
        }
        let report_relative = required_text(assignment, "report")?;
        safe_relative_path(report_relative)?;
        let report = read_json(&root.join(report_relative))?;
        validate_builder_report(&report, id)?;
        if report.get("language").and_then(Value::as_str) != Some(language)
            || report.get("brief_id").and_then(Value::as_str)
                != assignment.get("brief_id").and_then(Value::as_str)
        {
            return Err(format!(
                "{id}: report language or brief does not match its assignment"
            ));
        }
        for field in ["start_utc", "finish_utc"] {
            if assignment.get(field) != report.get(field) {
                return Err(format!("{id}: assignment and report {field} do not match"));
            }
        }
        for path in report
            .get("evidence_paths")
            .and_then(Value::as_array)
            .expect("builder report evidence_paths validated above")
        {
            let path = path
                .as_str()
                .ok_or_else(|| format!("{id}: evidence path must be text"))?;
            safe_relative_path(path)?;
            if !root.join(path).exists() {
                return Err(format!(
                    "{id}: evidence file or directory is missing: {path}"
                ));
            }
        }
        if assignment.get("status").and_then(Value::as_str) != Some("complete") {
            return Err(format!("{id}: assignment is not marked complete"));
        }
        let order = assignment
            .get("order_in_pair")
            .and_then(Value::as_u64)
            .filter(|order| (1..=2).contains(order))
            .ok_or_else(|| format!("{id}: order_in_pair must be 1 or 2"))?;
        let pair = brief_assignments
            .entry(brief.to_string())
            .or_insert_with(|| (class.to_string(), Vec::new()));
        if pair.0 != class {
            return Err(format!(
                "{brief}: pair assignments use different target classes"
            ));
        }
        pair.1.push((order, language.to_string()));
        seen_pairs.insert((class.to_string(), language.to_string()));
    }
    if brief_assignments.len() != CLASSES.len() {
        return Err("expected one distinct brief for each of the three target classes".into());
    }
    let recorded_pair_order = assignments
        .get("pair_order")
        .and_then(Value::as_object)
        .ok_or("builder assignments must record pair_order")?;
    if recorded_pair_order.len() != brief_assignments.len() {
        return Err("pair order must contain exactly one entry for each brief".into());
    }
    for (brief, (class, mut pair)) in brief_assignments {
        pair.sort_by_key(|(order, _)| *order);
        if pair.len() != LANGUAGES.len()
            || pair[0].0 != 1
            || pair[1].0 != 2
            || !LANGUAGES.contains(&pair[0].1.as_str())
            || !LANGUAGES.contains(&pair[1].1.as_str())
            || pair[0].1 == pair[1].1
        {
            return Err(format!(
                "{brief}: expected Python and Veritas once in pair order 1, 2"
            ));
        }
        let recorded = recorded_pair_order
            .get(&brief)
            .and_then(Value::as_array)
            .ok_or_else(|| format!("{brief}: missing recorded pair order"))?;
        let actual = [pair[0].1.as_str(), pair[1].1.as_str()];
        let seeded_order = match brief.as_str() {
            "invoice-sentinel" | "quote-merge" => ["python", "veritas"],
            "bookloom" => ["veritas", "python"],
            _ => unreachable!("unknown briefs are rejected below"),
        };
        if actual != seeded_order {
            return Err(format!(
                "{brief}: assignment order does not match the recorded seed"
            ));
        }
        if recorded.len() != actual.len()
            || recorded
                .iter()
                .zip(actual)
                .any(|(value, language)| value.as_str() != Some(language))
        {
            return Err(format!(
                "{brief}: recorded pair order does not match assignments"
            ));
        }
        let expected_class = match brief.as_str() {
            "invoice-sentinel" => "small-automation-cli",
            "quote-merge" => "concurrent-network",
            "bookloom" => "maintainable-local-app",
            _ => return Err(format!("{brief}: unknown builder brief")),
        };
        if class != expected_class {
            return Err(format!("{brief}: target class does not match its brief"));
        }
    }
    let reevaluation_randomization = assignments
        .get("reevaluation_randomization")
        .and_then(Value::as_object)
        .ok_or("builder assignments must record reevaluation_randomization")?;
    if reevaluation_randomization
        .get("seed")
        .and_then(Value::as_u64)
        != Some(20261029)
        || reevaluation_randomization
            .get("algorithm")
            .and_then(Value::as_str)
            .is_none_or(|value| !value.contains("Python random.Random shuffle"))
    {
        return Err(
            "clock remediation pair order must use the pre-recorded Python shuffle seed".into(),
        );
    }
    let reevaluation_list = assignments
        .get("reevaluations")
        .and_then(Value::as_array)
        .ok_or("builder assignments must contain a reevaluations array")?;
    if reevaluation_list.len() != LANGUAGES.len() {
        return Err("expected one post-clock reevaluation assignment per language".into());
    }
    let reevaluation_pair_order = assignments
        .get("reevaluation_pair_order")
        .and_then(Value::as_object)
        .ok_or("builder assignments must record reevaluation_pair_order")?;
    let recorded_order = reevaluation_pair_order
        .get(REEVALUATION_BRIEF)
        .and_then(Value::as_array)
        .ok_or("clock remediation brief is missing its randomized pair order")?;
    if reevaluation_pair_order.len() != 1
        || recorded_order.len() != LANGUAGES.len()
        || recorded_order[0].as_str() != Some("python")
        || recorded_order[1].as_str() != Some("veritas")
    {
        return Err("clock remediation pair order must match seed 20261029".into());
    }
    let product_change_ready = reevaluation_randomization
        .get("product_change_ready_utc")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or("clock remediation must record when the product change was ready")?;
    let mut seen_reevaluation_languages = BTreeSet::new();
    for assignment in reevaluation_list {
        let id = required_text(assignment, "assignment_id")?;
        if !seen_ids.insert(id.to_string()) {
            return Err(format!("duplicate assignment_id: {id}"));
        }
        let language = required_text(assignment, "language")?;
        if !LANGUAGES.contains(&language) || !seen_reevaluation_languages.insert(language) {
            return Err(format!("{id}: invalid or duplicate reevaluation language"));
        }
        if required_text(assignment, "brief_id")? != REEVALUATION_BRIEF
            || required_text(assignment, "target_class")? != "maintainable-local-app"
        {
            return Err(format!("{id}: reevaluation brief or class does not match"));
        }
        if assignment.get("context").and_then(Value::as_str) != Some("fresh")
            || assignment.get("status").and_then(Value::as_str) != Some("complete")
        {
            return Err(format!("{id}: reevaluation context or status is invalid"));
        }
        let private_directory = required_text(assignment, "private_work_directory")?;
        if !private_directory.starts_with("/private/tmp/veritas-fresh-builders/") {
            return Err(format!(
                "{id}: private work directory is outside the fresh-builder root"
            ));
        }
        if assignment
            .get("budget_minutes_including_followup")
            .and_then(Value::as_u64)
            != Some(15)
        {
            return Err(format!("{id}: reevaluation budget must be 15 minutes"));
        }
        if assignment
            .get("model")
            .and_then(Value::as_str)
            .is_some_and(|value| value != model)
            || assignment
                .get("reasoning_effort")
                .and_then(Value::as_str)
                .is_some_and(|value| value != effort)
        {
            return Err(format!(
                "{id}: reevaluation model and effort must match the baseline"
            ));
        }
        let start = required_text(assignment, "start_utc")?;
        let finish = required_text(assignment, "finish_utc")?;
        if start < product_change_ready || finish < start {
            return Err(format!(
                "{id}: reevaluation must run after the product change"
            ));
        }
        let order = assignment
            .get("order_in_pair")
            .and_then(Value::as_u64)
            .filter(|order| (1..=2).contains(order))
            .ok_or_else(|| format!("{id}: order_in_pair must be 1 or 2"))?;
        let expected_order = if language == "python" { 1 } else { 2 };
        if order != expected_order {
            return Err(format!(
                "{id}: reevaluation order does not match the recorded seed"
            ));
        }
        let report_relative = required_text(assignment, "report")?;
        safe_relative_path(report_relative)?;
        let report = read_json(&root.join(report_relative))?;
        validate_builder_report(&report, id)?;
        if report.get("language").and_then(Value::as_str) != Some(language)
            || report.get("brief_id").and_then(Value::as_str) != Some(REEVALUATION_BRIEF)
            || report.get("target_class").and_then(Value::as_str) != Some("maintainable-local-app")
            || report.get("start_utc").and_then(Value::as_str) != Some(start)
            || report.get("finish_utc").and_then(Value::as_str) != Some(finish)
        {
            return Err(format!(
                "{id}: reevaluation report does not match its assignment"
            ));
        }
        for path in report
            .get("evidence_paths")
            .and_then(Value::as_array)
            .expect("builder report evidence_paths validated above")
        {
            let path = path
                .as_str()
                .ok_or_else(|| format!("{id}: evidence path must be text"))?;
            safe_relative_path(path)?;
            if !root.join(path).exists() {
                return Err(format!("{id}: evidence path is missing: {path}"));
            }
        }
    }
    if seen_reevaluation_languages.len() != LANGUAGES.len() {
        return Err("clock remediation pair must contain Python and Veritas".into());
    }
    let assessment =
        read_json(&builder_root.join("reports/assessment-maintainable-local-app.json"))?;
    if assessment
        .get("reevaluation_brief_id")
        .and_then(Value::as_str)
        != Some(REEVALUATION_BRIEF)
        || assessment.get("product_change").and_then(Value::as_str) != Some("clock-local-date")
    {
        return Err(
            "maintainable-local-app assessment must incorporate the post-clock reevaluation".into(),
        );
    }
    for class in CLASSES {
        for language in LANGUAGES {
            if !seen_pairs.contains(&(class.to_string(), language.to_string())) {
                return Err(format!("missing {language} builder for {class}"));
            }
        }
        let assessment_path = builder_root.join(format!("reports/assessment-{class}.json"));
        let assessment = read_json(&assessment_path)?;
        if assessment.get("schema_version").and_then(Value::as_u64) != Some(1)
            || assessment.get("status").and_then(Value::as_str) != Some("complete")
            || assessment.get("target_class").and_then(Value::as_str) != Some(class)
            || assessment
                .get("assessed_after_both_outputs")
                .and_then(Value::as_bool)
                != Some(true)
        {
            return Err(format!("{class}: final assessment is incomplete"));
        }
        let recommendation = required_text(&assessment, "recommendation")?;
        if !["veritas", "python", "either"].contains(&recommendation) {
            return Err(format!(
                "{class}: recommendation must be veritas, python, or either"
            ));
        }
        if recommendation == "python" {
            return Err(format!("{class}: Python is preferred; map the specific gap to Tasks 2–10 and re-evaluate after the product change"));
        }
        if required_text(&assessment, "observed_user_benefit")?.is_empty()
            || assessment
                .get("material_regressions")
                .and_then(Value::as_array)
                .is_none()
            || assessment
                .get("evidence")
                .and_then(Value::as_array)
                .is_none()
        {
            return Err(format!(
                "{class}: assessment needs an observed benefit, regression list, and evidence list"
            ));
        }
    }
    Ok(CLASSES
        .iter()
        .map(|class| format!("assessed {class}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn write_audit_fixture(root: &Path, recorded_order: serde_json::Value) {
        let report_dir = root.join("evaluation/completion/builder/reports");
        std::fs::create_dir_all(&report_dir).unwrap();
        let classes = [
            (
                "small-automation-cli",
                "invoice-sentinel",
                ["python", "veritas"],
            ),
            ("concurrent-network", "quote-merge", ["python", "veritas"]),
            ("maintainable-local-app", "bookloom", ["veritas", "python"]),
        ];
        let mut assignments = Vec::new();
        for (class, brief, languages) in classes {
            for (index, language) in languages.into_iter().enumerate() {
                let id = format!("{brief}-{language}");
                let report_path = format!("evaluation/completion/builder/reports/{id}.json");
                assignments.push(json!({
                    "assignment_id": id,
                    "brief_id": brief,
                    "target_class": class,
                    "language": language,
                    "order_in_pair": index + 1,
                    "context": "fresh",
                    "status": "complete",
                    "private_work_directory": format!("/private/tmp/veritas-fresh-builders/{id}"),
                    "start_utc": "2026-10-25T09:00:00Z",
                    "finish_utc": "2026-10-25T09:05:00Z",
                    "budget_minutes_including_followup": 15,
                    "report": report_path
                }));
                std::fs::write(
                    root.join(&report_path),
                    json!({
                        "schema_version": 1,
                        "status": "complete",
                        "assignment_id": id,
                        "language": language,
                        "brief_id": brief,
                        "start_utc": "2026-10-25T09:00:00Z",
                        "finish_utc": "2026-10-25T09:05:00Z",
                        "budget_minutes": 15,
                        "elapsed_seconds": 300.0,
                        "initial_correct": true,
                        "followup_correct": true,
                        "relocation_passed": true,
                        "setup_seconds": 1,
                        "build_seconds": 2,
                        "followup_seconds": 3,
                        "installed_launch_seconds": 0.4,
                        "artifact_bytes": 1024,
                        "interventions": [],
                        "failures": [],
                        "recommendation": {
                            "preferred_language": "veritas",
                            "reason": "less setup",
                            "observed_user_benefit": "one installed command",
                            "material_regressions": []
                        },
                        "evidence_paths": [report_path]
                    })
                    .to_string(),
                )
                .unwrap();
            }
            std::fs::write(
                report_dir.join(format!("assessment-{class}.json")),
                json!({
                    "schema_version": 1,
                    "status": "complete",
                    "target_class": class,
                    "assessed_after_both_outputs": true,
                    "recommendation": "veritas",
                    "observed_user_benefit": "lower setup effort",
                    "material_regressions": [],
                    "evidence": ["both installed applications"],
                    "reevaluation_brief_id": "field-kit-log",
                    "product_change": "clock-local-date"
                })
                .to_string(),
            )
            .unwrap();
        }
        let mut reevaluations = Vec::new();
        for (order, language) in [(1, "python"), (2, "veritas")] {
            let id = format!("field-kit-log-{language}");
            let report_path = format!("evaluation/completion/builder/reports/{id}.json");
            let start = format!("2026-10-26T10:0{order}:00Z");
            let finish = format!("2026-10-26T10:0{}:00Z", order + 1);
            reevaluations.push(json!({
                "assignment_id": id,
                "brief_id": "field-kit-log",
                "target_class": "maintainable-local-app",
                "language": language,
                "order_in_pair": order,
                "context": "fresh",
                "status": "complete",
                "private_work_directory": format!("/private/tmp/veritas-fresh-builders/{id}"),
                "budget_minutes_including_followup": 15,
                "start_utc": start,
                "finish_utc": finish,
                "report": report_path
            }));
            std::fs::write(
                root.join(&report_path),
                json!({
                    "schema_version": 1,
                    "status": "complete",
                    "assignment_id": id,
                    "language": language,
                    "brief_id": "field-kit-log",
                    "target_class": "maintainable-local-app",
                    "start_utc": start,
                    "finish_utc": finish,
                    "budget_minutes": 15,
                    "elapsed_seconds": 300.0,
                    "initial_correct": true,
                    "followup_correct": true,
                    "relocation_passed": true,
                    "setup_seconds": 1,
                    "build_seconds": 2,
                    "followup_seconds": 3,
                    "installed_launch_seconds": 0.4,
                    "artifact_bytes": 1024,
                    "interventions": [],
                    "failures": [],
                    "recommendation": {
                        "preferred_language": "either",
                        "reason": "matched outcomes",
                        "observed_user_benefit": "local date is automatic",
                        "material_regressions": []
                    },
                    "evidence_paths": [report_path]
                })
                .to_string(),
            )
            .unwrap();
        }
        std::fs::write(
            root.join("evaluation/completion/builder/assignments.json"),
            json!({
                "schema_version": 1,
                "model": "gpt-6.1-sol",
                "reasoning_effort": "medium",
                "randomization": {
                    "algorithm": "Python random.Random shuffle, one shuffle per brief, seed 20261025",
                    "seed": 20261025
                },
                "pair_order": recorded_order,
                "assignments": assignments,
                "reevaluation_randomization": {
                    "algorithm": "Python random.Random shuffle with seed 20261029",
                    "seed": 20261029,
                    "product_change_ready_utc": "2026-10-26T10:00:00Z"
                },
                "reevaluation_pair_order": {"field-kit-log": ["python", "veritas"]},
                "reevaluations": reevaluations
            })
            .to_string(),
        )
        .unwrap();
    }

    #[test]
    fn builder_report_requires_followup_and_relocation_evidence() {
        let report = json!({
            "schema_version": 1,
            "status": "complete",
            "assignment_id": "invoice-sentinel-veritas",
            "language": "veritas",
            "brief_id": "invoice-sentinel",
            "start_utc": "2026-10-25T09:00:00Z",
            "finish_utc": "2026-10-25T09:01:00Z",
            "budget_minutes": 15,
            "elapsed_seconds": 60.0,
            "initial_correct": true,
            "followup_correct": true,
            "relocation_passed": false,
            "recommendation": "veritas"
        });

        assert!(
            super::validate_builder_report(&report, "invoice-sentinel-veritas")
                .unwrap_err()
                .contains("relocation")
        );
    }

    #[test]
    fn valid_builder_report_requires_all_measures_and_recommendation() {
        let report = json!({
            "schema_version": 1,
            "status": "complete",
            "assignment_id": "invoice-sentinel-veritas",
            "language": "veritas",
            "brief_id": "invoice-sentinel",
            "start_utc": "2026-10-25T09:00:00Z",
            "finish_utc": "2026-10-25T09:08:00Z",
            "budget_minutes": 15,
            "elapsed_seconds": 480.0,
            "initial_correct": true,
            "followup_correct": true,
            "relocation_passed": true,
            "setup_seconds": 10.0,
            "build_seconds": 100.0,
            "followup_seconds": 50.0,
            "installed_launch_seconds": 0.4,
            "artifact_bytes": 1024,
            "interventions": [],
            "failures": [],
            "recommendation": {
                "preferred_language": "veritas",
                "reason": "The delivered build is portable.",
                "observed_user_benefit": "One installed command builds and runs the app.",
                "material_regressions": []
            },
            "evidence_paths": ["evaluation/completion/builder/reports/evidence/sample"]
        });
        assert!(super::validate_builder_report(&report, "invoice-sentinel-veritas").is_ok());
    }

    #[test]
    fn audit_rejects_pair_order_that_does_not_match_recorded_assignments() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("veritas-builder-audit-{nonce}"));
        let wrong_order = json!({
            "invoice-sentinel": ["veritas", "python"],
            "quote-merge": ["python", "veritas"],
            "bookloom": ["python", "veritas"]
        });
        write_audit_fixture(&root, wrong_order);

        let error = super::audit(&root).unwrap_err();
        assert!(error.contains("pair order"), "{error}");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn audit_accepts_three_complete_matched_pairs() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("veritas-builder-audit-valid-{nonce}"));
        let order = json!({
            "invoice-sentinel": ["python", "veritas"],
            "quote-merge": ["python", "veritas"],
            "bookloom": ["veritas", "python"]
        });
        write_audit_fixture(&root, order);

        let result = super::audit(&root).unwrap();
        assert_eq!(result.len(), 3);
        std::fs::remove_dir_all(root).unwrap();
    }
}
