use serde::{Deserialize, Serialize};

pub const REPORT_FORMULA: &str = "score-v3";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicReport {
    pub formula_version: String,
    pub model_id: String,
    pub difficulty: String,
    pub speech_rate: u32,
    pub personal: f64,
    pub program: f64,
    pub semantic: f64,
    pub team_winner: String,
    pub standard: bool,
    pub non_standard_reason: String,
    pub length: f64,
    pub input_control: f64,
    pub execution: f64,
    pub trace: String,
    pub scoring_status: String,
    pub low_confidence: bool,
    pub dimensions: Vec<ReportDimension>,
    #[serde(default)]
    pub stage_evaluations: Vec<ReportStageEvaluation>,
    #[serde(default)]
    pub review_summary: String,
    #[serde(default)]
    pub team_summary: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportDimension {
    pub name: String,
    pub raw: f64,
    pub points: f64,
    pub transcript_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportStageEvaluation {
    pub transcript_id: String,
    pub strength: String,
    pub improvement: String,
    pub material_support: String,
    pub confidence: u32,
}

pub fn export_public(report: &PublicReport) -> String {
    serde_json::to_string(report).unwrap_or_else(|_| "{}".to_owned())
}

pub fn trend(reports: &[PublicReport]) -> Vec<&PublicReport> {
    reports.iter().filter(|report| report.standard).collect()
}

pub fn shown_personal(report: &PublicReport) -> f64 {
    report.personal
}

#[cfg(test)]
mod tests {
    use super::{export_public, shown_personal, trend, PublicReport, REPORT_FORMULA};

    fn report(personal: f64, standard: bool) -> PublicReport {
        PublicReport {
            formula_version: REPORT_FORMULA.to_owned(),
            model_id: "deepseek-flash".to_owned(),
            difficulty: "标准".to_owned(),
            speech_rate: 240,
            personal,
            program: 16.0,
            semantic: personal - 16.0,
            team_winner: "tie".to_owned(),
            standard,
            non_standard_reason: if standard {
                String::new()
            } else {
                "恢复".to_owned()
            },
            length: 8.0,
            input_control: 8.0,
            execution: 0.0,
            trace: "length=8".to_owned(),
            scoring_status: "ready".to_owned(),
            low_confidence: false,
            dimensions: Vec::new(),
            stage_evaluations: Vec::new(),
            review_summary: String::new(),
            team_summary: String::new(),
        }
    }

    #[test]
    fn export_keeps_secrets_out_and_old_scores_stay_stored() {
        let stored = report(81.2, true);
        let exported = export_public(&stored);
        assert!(!exported.contains("api_key"));
        assert!(!exported.contains("keystroke"));
        assert!(!exported.contains("反方私有"));
        assert_eq!(shown_personal(&stored), 81.2);
        let mut newer = stored.clone();
        newer.formula_version = "program-v2".to_owned();
        newer.personal = 90.0;
        assert_eq!(shown_personal(&stored), 81.2);
        assert_eq!(trend(&[stored, report(10.0, false)]).len(), 1);
    }
}
